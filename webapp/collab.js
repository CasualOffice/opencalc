// The browser half of collaboration: a socket, and the protocol over it.
//
// Everything under this file is Rust and gated — the transform, the client
// session, the wire format, the server. This is the part that carries messages
// between them, and it owns exactly three things the engine deliberately does
// not: the socket, the reconnect, and the backoff.
//
// It is a module rather than part of editor.js because it has no opinions about
// a grid. It is handed the wasm exports and a set of callbacks, and a host that
// is not this editor can use it unchanged.

/// How long to wait before the first reconnect, and the ceiling it doubles to.
const RETRY_FLOOR_MS = 250;
const RETRY_CEILING_MS = 15_000;

/// How often to send an application heartbeat.
//
// Separate from the WebSocket pings the server sends, which the browser answers
// without asking us. This one is what keeps the *cursor* alive: presence
// expires on silence, and a participant reading rather than typing is still
// present.
const HEARTBEAT_MS = 10_000;

/// How long local edits accumulate before being sent.
//
// Not zero. A submission per keystroke is a submission per keystroke for every
// other participant to apply, and the transform cost is per operation. Small
// enough that somebody typing sees their work land continuously.
const FLUSH_MS = 120;

/// An opaque key for this editor's participation.
///
/// `randomUUID` where it exists, which is everywhere a browser supports
/// WebSockets and secure contexts. The fallback is not a security measure — the
/// key is not a credential — only enough uniqueness that two tabs do not
/// collide.
function newKey() {
  if (globalThis.crypto?.randomUUID) return globalThis.crypto.randomUUID();
  return `k-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

/// Join a collaborative session.
///
/// `wasm` is the engine module. `onStatus`, `onDocument` and `onPresence` are
/// how the host learns what happened; none of them is optional, because a
/// caller that ignores a status is a caller that shows a stale document
/// forever.
export function collaborate({ url, token, document: documentKey, wasm, onStatus, onDocument, onPresence }) {
  let socket = null;
  let closed = false;
  let retryMs = RETRY_FLOOR_MS;
  let heartbeat = null;
  let flusher = null;
  // Set once a Welcome or a Resumed has arrived. Until then nothing may be
  // sent: the engine's session does not exist and a submission would be against
  // a revision nobody agreed on.
  let joined = false;

  // This editor's claim to be the participant it was, for reconnecting.
  //
  // Generated once and held for the life of this session — deliberately *not*
  // derived from the user, because the same person with the same token in two
  // tabs is two participants, and sharing an id would have each tab's
  // submissions suppress the other's as duplicates.
  //
  // Not a secret and not treated as one: the token is what authorises, and the
  // server only honours a key for the user it issued it to. See ADR-015.
  const resumeKey = newKey();

  /// Where the engine believes the document is. Sent on a reconnect so the
  /// server knows what to catch us up on.
  let revision = 0;
  /// Set when a resume succeeded and the chunk in flight has not been sent
  /// again yet. The resend has to wait for the missed operations to be applied,
  /// because they are what rebase it.
  let mustResend = false;

  const status = (state, detail) => onStatus({ state, detail });

  function open() {
    if (closed) return;
    const target = new URL(url);
    target.searchParams.set("doc", documentKey);
    socket = new WebSocket(target);

    socket.onopen = () => {
      status("connecting");
      send({
        type: "join",
        protocol: wasm.protocol_version(),
        token,
        // Offered from the first connection onward, because the point of a key
        // is the *next* one. A server that has never seen it treats this as an
        // ordinary join.
        resume: { key: resumeKey, revision },
      });
    };

    socket.onmessage = (event) => {
      let message;
      try {
        message = JSON.parse(event.data);
      } catch {
        // A frame this cannot read is not a reason to drop a working session;
        // a later one may be fine, and the server is not a hostile party.
        return;
      }
      receive(message);
    };

    socket.onclose = () => {
      stopTimers();
      joined = false;
      if (closed) return;
      // Full jitter. Without it every client of a server that restarted
      // reconnects at the same instant and knocks it over again — and the
      // second outage is caused by the clients, which is harder to diagnose
      // than the first.
      const wait = Math.random() * retryMs;
      retryMs = Math.min(retryMs * 2, RETRY_CEILING_MS);
      status("reconnecting", Math.round(wait));
      setTimeout(open, wait);
    };

    // `onerror` is followed by `onclose`, so the reconnect is handled once,
    // there. Acting on both schedules two.
    socket.onerror = () => {};
  }

  function receive(message) {
    switch (message.type) {
      case "welcome": {
        if (message.protocol !== wasm.protocol_version()) {
          // Should be unreachable — the server checks this before it welcomes
          // anyone. Checked anyway because proceeding means applying a
          // document from a peer that does not agree what the words mean.
          status("stopped", "protocolVersion");
          close();
          break;
        }
        // The snapshot is the document as everyone else has it *at this
        // revision*. Loading the file instead would start this participant at
        // revision zero while the session was at five hundred.
        //
        // `Uint8Array`, not the array JSON gave us: a `Vec<u8>` crosses as an
        // array of numbers, and the binding takes a byte slice. Handing it the
        // plain array throws at the boundary rather than converting.
        wasm.session_load_snapshot(new Uint8Array(message.snapshot));
        wasm.collab_begin(message.client, message.revision);
        joined = true;
        revision = message.revision;
        mustResend = false;
        retryMs = RETRY_FLOOR_MS;
        onDocument({ reason: "joined", revision: message.revision, editable: message.editable });
        status("live");
        startTimers();
        break;
      }
      case "resumed": {
        // No snapshot, and that is the whole point: this client's document is
        // continuous and may hold edits the server never received. Replacing it
        // would discard exactly the unsent work resuming exists to preserve.
        wasm.collab_resume(message.client, revision);
        for (const op of message.missed) {
          wasm.collab_receive(JSON.stringify(op), message.revision);
        }
        revision = message.revision;
        joined = true;
        // Now, and not before: the chunk in flight had to be rebased past what
        // arrived above, or it would be submitted against a revision the server
        // has moved beyond.
        mustResend = true;
        retryMs = RETRY_FLOOR_MS;
        onDocument({ reason: "resumed", revision, editable: message.editable });
        status("live");
        startTimers();
        flush();
        break;
      }
      case "ack":
        wasm.collab_acknowledge(message.revision);
        revision = message.revision;
        break;
      case "apply":
        for (const op of message.ops) {
          wasm.collab_receive(JSON.stringify(op), message.revision);
        }
        revision = message.revision;
        onDocument({ reason: "remote", revision: message.revision });
        break;
      case "presence":
        onPresence({ kind: "here", ...message });
        break;
      case "departed":
        onPresence({ kind: "gone", client: message.client });
        break;
      case "refused":
        // A refusal is about one submission, or about the session as a whole
        // when it names none. Either way the connection survives: being told
        // "not saving" is precisely the moment a user wants their document
        // still on screen so they can copy out of it.
        status("refused", why(message.reason));
        break;
      case "stopped":
        // The server is finished with this session. Reconnecting would be
        // refused for the same reason, so stop rather than loop.
        status("stopped", why(message.reason));
        close();
        break;
      default:
        break;
    }
  }

  function startTimers() {
    stopTimers();
    heartbeat = setInterval(() => send({ type: "heartbeat" }), HEARTBEAT_MS);
    flusher = setInterval(flush, FLUSH_MS);
  }

  function stopTimers() {
    clearInterval(heartbeat);
    clearInterval(flusher);
    heartbeat = null;
    flusher = null;
  }

  function flush() {
    if (!joined || socket?.readyState !== WebSocket.OPEN) return;
    if (mustResend) {
      mustResend = false;
      // The *same* sequence number as before. That is what lets the server
      // recognise a chunk it already committed and answer with where it landed,
      // rather than applying it a second time — which, for an insert-rows, is
      // corruption rather than a glitch.
      const again = wasm.collab_resend();
      if (again) {
        socket.send(again);
        return;
      }
    }
    const submission = wasm.collab_flush();
    // Empty means nothing to send *or* a chunk already in flight. One at a
    // time, by design: with two outstanding, an acknowledgement does not say
    // which it was for.
    if (submission) socket.send(submission);
  }

  function send(message) {
    if (socket?.readyState === WebSocket.OPEN) socket.send(JSON.stringify(message));
  }

  /// Send local presence. Carries no identity — the server takes that from the
  /// token, because presence is the one surface where a claimed name would be
  /// believed.
  function present(sheet, selection) {
    send({ type: "presence", sheet, selection });
  }

  /// The name of a refusal.
  ///
  /// A `Refusal` is tagged on its own `reason` key, so the reason *of* a
  /// message that has a `reason` field is one level further in. Reading the
  /// field directly yields the object, and a status line that says
  /// `[object Object]` is how a user learns nothing.
  function why(reason) {
    return reason?.reason ?? "unknown";
  }

  function close() {
    closed = true;
    stopTimers();
    // Said out loud rather than left to a dropped socket. Both work, but a
    // silent disconnect leaves this participant's cursor on everyone else's
    // screen until presence expires, which looks like someone who is still
    // there and is not.
    send({ type: "leave" });
    wasm.collab_end();
    socket?.close();
    socket = null;
  }

  /// Drop the connection and start a new one now.
  ///
  /// For a host that knows something the socket does not: a network changed, a
  /// laptop woke, a VPN came up. A dead TCP connection can look open for
  /// minutes before the operating system gives up on it, and every one of those
  /// minutes is a user typing into an editor that is quietly not sending
  /// anything. The reconnect resumes, so nothing typed in the meantime is lost.
  function reconnect() {
    // `close()` on the socket, not on this session: `onclose` then schedules
    // the reconnect through the same path a real drop takes, which is what
    // makes this exercise the real thing rather than a shortcut around it.
    socket?.close();
  }

  open();
  return { present, close, flush, reconnect };
}
