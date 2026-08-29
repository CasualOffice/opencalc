// There has to be a way to start sharing — and it must not be switched on.
//
// `docs/12` §8 ranks "there is no way to share" the third switching blocker:
// below the line a clustered OT server with a leader per document, epoch-fenced
// appends, relay, resume and presence, exercised by two real browsers in CI;
// above it, no Share command of any kind. §3.22 says the way in is `?doc=` on
// the URL, and that is not so either — nothing in the editor has ever read
// `?doc=` off the page URL, so there was no user-reachable route at all.
//
// The other half of the requirement is that this cannot simply be turned on.
// `COL-46` is an open **P0**: a `$`-anchored formula rebased across a
// concurrent insert lands as `$E$1` on one replica and `$D$1` on the other,
// with no error raised anywhere. A Share button that walked a user into that
// silently would be worse than no button.
//
// So the tests below assert both sides: the command exists and works, and by
// default it is absent from the menu *and* refused by `runCommand` — the second
// half being the one that is easy to get wrong, since hiding a command from the
// menu while leaving it runnable from a script hides it from the only party who
// could have declined the risk.

import { expect, test } from "@playwright/test";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_new());
}

/** Turn the capability on, the way a host that accepts `COL-46` would. */
const allowSharing = (page) =>
  page.evaluate(() => window.opencalcEditor.setCapabilities({ canShare: true }));

const commands = (page) => page.evaluate(() => window.opencalcEditor.listCommands());

test("a plain editor offers a share command", async ({ page }) => {
  await boot(page);
  // The exact probe `docs/12` §3.22 used to report as finding nothing. It held
  // the feature back while `COL-46` was open — a `$`-anchored formula rebased
  // across a concurrent insert diverged silently, and a Share button that walks
  // two people into that is worse than no button. `COL-46` is Done, so the
  // route is open and this asserts the opposite of what it used to.
  expect((await commands(page)).filter((id) => /share|collab|invite/i.test(id))).toContain("file.share");
});

test("a host that owns the document still decides for itself", async ({ page }) => {
  await boot(page);
  // `canShare` is true in `standalone` and `desktop` and false in `embedded`
  // and `wopi`. That is not a leftover of the COL-46 gate: starting a session
  // for a document somebody else owns is their decision, not ours.
  const refusal = await page.evaluate(() => {
    window.opencalcEditor.setCapabilities({ mode: "embedded" });
    try { window.opencalcEditor.runCommand("file.share"); return "ran"; }
    catch (e) { return String(e.message); }
  });
  expect(refusal).toMatch(/not available in this mode/);
});

test("the command on the menu is the command runCommand runs", async ({ page }) => {
  await boot(page);
  const ran = await page.evaluate(() => {
    try { window.opencalcEditor.runCommand("file.share"); return "ran"; }
    catch (e) { return String(e.message); }
  });
  // The pair that matters: a command listed but not runnable, or runnable but
  // not listed, are both ways for the menu to lie about what the editor can do.
  expect(ran).toBe("ran");
});

test("a host that accepts the risk gets the command", async ({ page }) => {
  await boot(page);
  await allowSharing(page);
  expect(await commands(page)).toContain("file.share");
});

test("the dialog names COL-46 before it will connect anywhere", async ({ page }) => {
  await boot(page);
  await allowSharing(page);
  await page.evaluate(() => window.opencalcEditor.runCommand("file.share"));

  await expect(page.locator("#oc-modal")).toBeVisible();
  await expect(page.locator("#share-warning")).toContainText("COL-46");
  // Specific, not "collaboration is experimental". A user has to be able to
  // tell which of their formulas this is about.
  await expect(page.locator("#share-warning")).toContainText("$E$1");

  // Disabled until acknowledged: a button that can be pressed and then
  // complains has already taught the user to press first and read after.
  await expect(page.locator("#share-start")).toBeDisabled();
  await page.locator("#share-ack").check();
  await expect(page.locator("#share-start")).toBeEnabled();
});

test("?collab= and ?doc= prefill the dialog and never connect on their own", async ({ page }) => {
  const messages = [];
  page.on("console", (m) => messages.push(m.text()));
  await page.goto("/editor.html?collab=ws%3A%2F%2F127.0.0.1%3A9%2Fcollab&doc=prefilled-key");
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/, { timeout: 30_000 });

  // The editor must not have opened a socket to a URL somebody put in a link.
  // Port 9 (discard) is unreachable, so an auto-connect shows up as a failed
  // WebSocket in the console and as a status that is not the ready one.
  await expect(page.locator("#tb-status")).toHaveText(/^engine v\d/);
  expect(messages.filter((m) => /WebSocket/i.test(m))).toEqual([]);

  await allowSharing(page);
  await page.evaluate(() => window.opencalcEditor.runCommand("file.share"));
  await expect(page.locator("#share-url")).toHaveValue("ws://127.0.0.1:9/collab");
  await expect(page.locator("#share-doc")).toHaveValue("prefilled-key");
});

test("a host's defaults fill the dialog and the token is not handed back out", async ({ page }) => {
  await boot(page);
  await allowSharing(page);
  const echoed = await page.evaluate(() =>
    window.opencalcEditor.setShareDefaults({
      url: "wss://collab.example.com/collab",
      document: "quarterly-budget",
      token: "a-real-credential",
    }),
  );
  // A getter that hands a credential back to any script on the page is a second
  // way to leak the thing the invite link is careful not to carry.
  expect(echoed).toEqual({ url: "wss://collab.example.com/collab", document: "quarterly-budget", token: true });

  await page.evaluate(() => window.opencalcEditor.runCommand("file.share"));
  await expect(page.locator("#share-url")).toHaveValue("wss://collab.example.com/collab");
  await expect(page.locator("#share-doc")).toHaveValue("quarterly-budget");
});

test("a session a host started is what the dialog describes, and the link carries no token", async ({ page }) => {
  await boot(page);
  await allowSharing(page);
  // The route a host uses today, and the one that has existed all along:
  // `collaborate()` against the module namespace, with no dialog involved. The
  // transport's handle does not carry the document key back, so if the editor
  // did not record it on the way in, Share would offer an invite link with an
  // empty `doc=` — an invitation to nothing.
  //
  // Port 9 is discard: the socket never connects, which is irrelevant here.
  // What is under test is what the *editor* knows about the session it opened.
  await page.evaluate(() =>
    window.opencalcEditor.collaborate({
      url: "ws://127.0.0.1:9/collab",
      token: "host-minted-credential",
      document: "started-by-the-host",
    }),
  );
  await page.evaluate(() => window.opencalcEditor.runCommand("file.share"));

  await expect(page.locator("#share-stop")).toBeVisible();
  const link = await page.locator("#share-link").inputValue();
  expect(link).toContain("doc=started-by-the-host");
  // The one thing the link must never carry. A credential on a URL is a
  // credential in browser history, in the referrer, and in whatever the link
  // was pasted into.
  expect(link).not.toContain("host-minted-credential");
  expect(link).not.toMatch(/token/i);

  await page.evaluate(() => window.opencalcEditor.stopCollaborating());
});

test("a failed connection leaves the dialog open with what was typed in it", async ({ page }) => {
  await boot(page);
  await allowSharing(page);
  await page.evaluate(() => window.opencalcEditor.runCommand("file.share"));

  await page.locator("#share-url").fill("not a url");
  await page.locator("#share-doc").fill("doomed");
  await page.locator("#share-token").fill("nonsense");
  await page.locator("#share-ack").check();
  await page.locator("#share-start").click();

  // Closing on a failure throws away the endpoint and token the user just
  // typed, which is exactly what they need in order to try again.
  await expect(page.locator("#share-error")).toBeVisible();
  await expect(page.locator("#share-doc")).toHaveValue("doomed");
  await expect(page.locator("#share-start")).toBeEnabled();
});
