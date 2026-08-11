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
  // Set once a Welcome has arrived. Until then nothing may be sent: the
  // engine's session does not exist and a submission would be against a
  // revision nobody agreed on.
  let joined = false;

  const status = (state, detail) => onStatus({ state, detail });

  function open() {
    if (closed) return;
    const target = new URL(url);
    target.searchParams.set("doc", documentKey);
    socket = new WebSocket(target);

    socket.onopen = () => {
      status("connecting");
      send({ type: "join", protocol: wasm.protocol_version(), token });
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
        // The snapshot is the document as everyone else has it *at this
        // revision*. Loading the file instead would start this participant at
        // revision zero while the session was at five hundred.
        wasm.session_load_snapshot(message.snapshot);
        wasm.collab_begin(message.client, message.revision);
        joined = true;
        retryMs = RETRY_FLOOR_MS;
        onDocument({ reason: "joined", revision: message.revision, editable: message.editable });
        status("live");
        startTimers();
        break;
      }
      case "ack":
        wasm.collab_acknowledge(message.revision);
        break;
      case "apply":
        for (const op of message.ops) {
          wasm.collab_receive(JSON.stringify(op), message.revision);
        }
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
        status("refused", message.reason);
        break;
      case "stopped":
        // The server is finished with this session. Reconnecting would be
        // refused for the same reason, so stop rather than loop.
        status("stopped", message.reason);
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

  function close() {
    closed = true;
    stopTimers();
    wasm.collab_end();
    socket?.close();
    socket = null;
  }

  open();
  return { present, close, flush };
}
