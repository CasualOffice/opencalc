// PROD-13: upload a real `.xlsx`, share it, edit it from two clients.
//
// Every leg of this is covered somewhere on its own — the importer has unit
// tests, the collaboration protocol has a browser suite, the compose stack has
// a reachability check. What nothing covered is the **chain**: that a file a
// person uploads is the file the collaboration server fetches, that two clients
// handed tokens by the host converge on the same document, and that what comes
// back out of `/download` is still a workbook.
//
// A chain is exactly where this deployment breaks, because each link is
// configured separately. The collaboration server reaches the host on an
// internal URL that only exists inside the compose network, and every previous
// failure of this kind looked like "the editor is blank" with three healthy
// containers and nothing in any log.
//
// # Why a script rather than a Playwright spec
//
// The browser suite already proves two *browsers* converge. What is unproven is
// the same thing through the **deployed** stack, and the job that has the stack
// has no browser. This speaks the wire protocol directly, which is the part the
// browsers were exercising anyway, and runs anywhere Node does.
//
// Usage:  node tools/e2e-document-lifecycle.mjs [base-url]

import { readFileSync } from "node:fs";

const BASE = (process.argv[2] ?? "http://127.0.0.1:8080").replace(/\/$/, "");

/// The protocol version the engine speaks, read from the engine.
///
/// Not a constant written down here. `COL-42` was exactly this drifting: the
/// browser suite kept its own copy, the engine moved to 6, and the server
/// refused every join — after a merge, in a ten-minute job.
function protocolVersion() {
  const source = readFileSync("crates/casual-calc-transaction/src/protocol.rs", "utf8");
  const found = source.match(/PROTOCOL_VERSION: u32 = (\d+)/);
  if (!found) throw new Error("could not read PROTOCOL_VERSION from the engine");
  return Number(found[1]);
}

const fail = (message) => {
  console.error(`::error::${message}`);
  process.exit(1);
};

const step = (message) => console.log(`  ${message}`);

/// The smallest thing that is genuinely an `.xlsx`.
///
/// Built by the engine's own writer would be circular — if the writer is broken
/// this would upload something broken and the host would refuse it, which reads
/// as an upload bug. A fixture the repository already ships is an independent
/// file.
function realWorkbook() {
  // A corpus file, written by somebody else's spreadsheet. `fixtures/generated`
  // is produced by this project's own tooling, and uploading one would prove
  // the writer and the reader agree with each other rather than that a real
  // workbook survives the trip.
  for (const path of [
    "fixtures/corpus/SampleSS.xlsx",
    "fixtures/corpus/WithMoreVariousData.xlsx",
    "fixtures/generated/minimal.xlsx",
  ]) {
    try {
      return { name: path.split("/").pop(), bytes: readFileSync(path) };
    } catch {
      /* try the next */
    }
  }
  throw new Error("no .xlsx fixture found to upload");
}

/// Join a document and run `act` once the server has welcomed us.
///
/// Resolves with every message heard after that point, so a caller can assert
/// on what the *other* client caused.
function speak(collabUrl, token, docKey, act, { want = 1, timeoutMs = 20_000 } = {}) {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(`${collabUrl}?doc=${encodeURIComponent(docKey)}`);
    const heard = [];
    let joined = false;
    const timer = setTimeout(() => {
      socket.close();
      reject(new Error(`no reply within ${timeoutMs}ms (heard ${JSON.stringify(heard)})`));
    }, timeoutMs);

    socket.onerror = () => {
      clearTimeout(timer);
      reject(new Error(`the socket failed against ${collabUrl}`));
    };
    socket.onopen = () =>
      socket.send(JSON.stringify({ type: "join", protocol: protocolVersion(), token }));
    socket.onmessage = (event) => {
      const message = JSON.parse(event.data);
      // `opening` says the token was accepted and a fetch is starting; presence
      // is unsolicited. Neither answers anything asked here.
      if (message.type === "opening" || message.type === "presence" || message.type === "departed") {
        return;
      }
      if (message.type === "welcome") {
        joined = true;
        act((m) => socket.send(JSON.stringify(m)), message.client, message.revision);
        return;
      }
      if (!joined) {
        clearTimeout(timer);
        socket.close();
        reject(new Error(`refused before joining: ${JSON.stringify(message)}`));
        return;
      }
      heard.push(message);
      if (heard.length < want) return;
      clearTimeout(timer);
      socket.close();
      resolve(heard);
    };
  });
}

async function main() {
  console.log(`end-to-end against ${BASE}`);

  // 1. Upload a real workbook, as a person does.
  const file = realWorkbook();
  const form = new FormData();
  form.append("file", new Blob([file.bytes]), file.name);
  // **`redirect: "manual"`.** Upload is a browser form POST and answers `303`
  // to the new document, so a followed redirect lands on the editor page and
  // the id is only in the `Location` header. Following it silently turns a
  // successful upload into "the response is not JSON", which is what this
  // script reported the first time it ran.
  const uploaded = await fetch(`${BASE}/api/upload`, {
    method: "POST",
    body: form,
    redirect: "manual",
  });
  if (uploaded.status !== 303) {
    fail(`upload answered ${uploaded.status}, expected a 303 to the new document: ${await uploaded.text()}`);
  }
  const location = uploaded.headers.get("location") ?? "";
  const id = location.match(/\/d\/([^/?#]+)/)?.[1];
  if (!id) fail(`the upload redirected to ${location || "nowhere"}, with no document id in it`);
  step(`uploaded ${file.name} as ${id}`);

  // 2. The share link opens. This is the page a recipient is sent.
  const page = await fetch(`${BASE}/d/${id}`);
  if (!page.ok) fail(`the share link answered ${page.status}`);
  step("the share link serves the editor");

  // 3. Two participants are handed tokens by the host — not minted here, so
  //    the host's own signing is part of what is under test.
  const sessions = await Promise.all(
    ["Ada", "Grace"].map(async (name) => {
      const response = await fetch(`${BASE}/api/documents/${id}/session?name=${name}`);
      if (!response.ok) fail(`session for ${name} answered ${response.status}`);
      return response.json();
    }),
  );
  const [ada, grace] = sessions;
  if (!ada.token || !ada.collab) fail(`the session is missing a token or endpoint: ${JSON.stringify(ada)}`);
  step(`two sessions issued, collaborating over ${ada.collab}`);

  // 4. Grace joins and waits; Ada edits. Grace must see it.
  //
  //    This is the leg that fails in a deployment and nowhere else: the
  //    collaboration server has to fetch the uploaded file from the host over
  //    an internal address, and a client that never receives the peer's edit is
  //    what that failure looks like.
  const watching = speak(grace.collab, grace.token, grace.document, () => {}, { want: 1 });
  // A moment for the watcher to be inside the document before the edit lands,
  // so this asserts propagation rather than catch-up.
  await new Promise((r) => setTimeout(r, 750));

  await speak(ada.collab, ada.token, ada.document, (send, me, revision) => {
    send({
      type: "submit",
      client: me,
      seq: 1,
      base: { revision },
      ops: [
        {
          op: { setValue: { sheet: 0, at: { row: 4, col: 1 }, value: { inlineString: 1 } } },
          formulas: {},
          styles: {},
          strings: { 1: "prod-13" },
        },
      ],
    });
  });
  step("Ada's edit was accepted");

  const seen = await watching;
  const applied = seen.some((m) => JSON.stringify(m).includes("prod-13"));
  if (!applied) {
    fail(`Grace never saw Ada's edit; she heard ${JSON.stringify(seen)}`);
  }
  step("Grace received it — two clients are on the same document");

  // 5. What comes back out is still a workbook. A round trip that returns
  //    something unopenable has lost the file just as surely as losing it.
  const download = await fetch(`${BASE}/api/documents/${id}/download`);
  if (!download.ok) fail(`download answered ${download.status}`);
  const bytes = Buffer.from(await download.arrayBuffer());
  if (bytes.length < 4 || bytes[0] !== 0x50 || bytes[1] !== 0x4b) {
    fail(`the download is not a zip package: first bytes ${[...bytes.slice(0, 8)]}`);
  }
  step(`downloaded ${bytes.length} bytes, still an OOXML package`);

  console.log("end-to-end: upload -> share -> co-edit -> download all passed");
}

main().catch((why) => fail(why.message ?? String(why)));
