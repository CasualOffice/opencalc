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

/// How often to ask the server whether this connection is still alive, and how
/// long to wait for the answer.
//
// This is the only thing on the client that can notice a **half-open**
// connection — a slept laptop, a vanished network, a load balancer that dropped
// the flow without saying so. Such a socket looks perfectly open to the browser
// holding it for as long as the operating system takes to give up, which is
// minutes; and for all of those minutes the editor works, accepts typing, and
// writes into nothing. The socket does not notice. The flush does not notice:
// `send` on a dead socket does not fail, it just goes nowhere.
//
// The deadline is generous next to the interval, because the cost of being
// wrong is asymmetric — a reconnect that was not needed is invisible, and one
// that was needed and did not happen is a user losing everything typed since.
// Since ADR-015 a reconnect resumes, so being wrong costs a round trip.
const PING_MS = 15_000;
const PONG_DEADLINE_MS = 10_000;

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
  /// Where this transport is connecting. Not the parameter, because a full node
  /// can name another one to try — see `redirect`.
  let endpoint = url;
  /// Whether a redirect has already been followed.
  ///
  /// **Exactly one, per session.** Two nodes that are both full would otherwise
  /// name each other and bounce a client between them for as long as it had the
  /// patience to keep asking. One hop reaches a node with room whenever one
  /// exists, because placement picks the least loaded of *all* peers rather
  /// than the next one along.
  let redirected = false;
  let heartbeat = null;
  let flusher = null;
  // Set once a Welcome or a Resumed has arrived. Until then nothing may be
  // sent: the engine's session does not exist and a submission would be against
  // a revision nobody agreed on.
  let joined = false;
  /// Whether this transport has ever completed a join.
  ///
  /// The difference between a first join and a reconnect, which is the
  /// difference between "there is nothing to lose" and "something was lost".
  /// `joined` is cleared on every disconnect and so cannot answer it.
  let everJoined = false;
  /// Whether a reconnect discarded unsent work and the user has not yet had an
  /// edit acknowledged since. Holds back "live" for exactly that long, so the
  /// notice is not erased by a socket that came back before the user looked.
  let lostUnsent = false;

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

  /// The ping timer, the nonce counter, and the ping still waiting for an
  /// answer — `{ nonce, sentAt }`, or null when nothing is outstanding.
  let pinger = null;
  let nonce = 0;
  let awaiting = null;
  /// The last measured round trip, for a host that wants to show it.
  let roundTripMs = null;

  const status = (state, detail) => onStatus({ state, detail });

  function open() {
    if (closed) return;
    const target = new URL(endpoint);
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
      case "opening":
        // Authorised, and the server is fetching the document from the host.
        // Reported rather than waited out silently: this can take as long as
        // the server's HTTP timeout allows, and silence on an open socket is
        // what a user reloads over — starting a second wait while the first is
        // still running.
        status("opening", message.title);
        // The pinger starts *here*, not at the welcome. This wait is the
        // longest stretch of the session with no traffic on it, so it is the
        // most likely place for a connection to die unnoticed — and without
        // this, the one state where "slow" and "broken" look identical would be
        // the one state where telling them apart matters most.
        startPinger();
        break;
      case "welcome": {
        if (message.protocol !== wasm.protocol_version()) {
          // Should be unreachable — the server checks this before it welcomes
          // anyone. Checked anyway because proceeding means applying a
          // document from a peer that does not agree what the words mean.
          status("stopped", "protocolVersion");
          close();
          break;
        }
        // **Asked before the snapshot lands, because afterwards there is
        // nothing left to ask about.** A welcome that follows a previous join
        // is a reconnect the server did not resume — the resume key was not
        // recognised, because the reconnect was balanced to another node, or
        // the node restarted, or the document was evicted, or the key aged out
        // of a bounded map. Whatever the reason, the snapshot below replaces
        // the whole workbook and the collaborative session with it, discarding
        // the applied-op log, the history, and every chunk still unacknowledged.
        //
        // docs/61 says that loss is "lost loudly". It was not: nothing here
        // consulted `collab_unacknowledged`, which exists to answer exactly
        // this question, and the server's `TooFarBehind` refusal is only sent
        // when it *recognises* the key — an unrecognised one produces no
        // refusal at all. So a user typing through a rolling deploy watched
        // their cells disappear under a status line reading "collaborating".
        //
        // Nothing here can save the work: those operations were written
        // against a revision this client no longer holds, and re-applying them
        // untransformed is the divergence the whole transform exists to
        // prevent. Announcing it is the honest remedy, and it has to happen
        // first.
        const losing = everJoined && wasm.collab_unacknowledged();
        if (losing) {
          lostUnsent = true;
          status("lost", "unsentEdits");
          onDocument({ reason: "lost", revision: message.revision });
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
        everJoined = true;
        revision = message.revision;
        mustResend = false;
        retryMs = RETRY_FLOOR_MS;
        onDocument({ reason: "joined", revision: message.revision, editable: message.editable });
        // Not overwritten by "live" when something was lost. A data-loss notice
        // that the next status line erases three microseconds later is a notice
        // nobody sees, which is how the `refused` state was already going
        // unnoticed on this same socket.
        if (!losing) status("live");
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
      case "pong":
        // Matched, not merely counted. A late answer to an earlier ping would
        // otherwise satisfy the current one, and a connection dying slowly
        // would read as healthy — which is exactly the connection this is for.
        if (awaiting?.nonce === message.nonce) {
          roundTripMs = Date.now() - awaiting.sentAt;
          awaiting = null;
        }
        break;
      case "ack":
        // Cumulative: this settles every chunk up to `through`, not only the
        // one it names. A skipped or lost acknowledgement is covered by the
        // next, rather than leaving a chunk outstanding with nothing to say so.
        wasm.collab_acknowledge(message.through, message.revision);
        revision = message.revision;
        // Work made *since* the loss has now landed on the server, which is the
        // first moment "collaborating" is true again rather than merely
        // connected. Until here the status line keeps saying what was lost.
        if (lostUnsent) {
          lostUnsent = false;
          status("live");
        }
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
        // refused for the same reason, so stop rather than loop — *unless* it
        // named somewhere else, which is the one case where the same request
        // to a different node gets a different answer (`DEP-09`).
        if (redirect(message.elsewhere)) break;
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
    startPinger();
  }

  /// Start the liveness check on its own, for the stretch before joining.
  function startPinger() {
    clearInterval(pinger);
    pinger = setInterval(ping, PING_MS);
  }

  function stopTimers() {
    clearInterval(heartbeat);
    clearInterval(flusher);
    clearInterval(pinger);
    heartbeat = null;
    flusher = null;
    pinger = null;
    // Cleared with the timers. A ping outstanding across a reconnect would be
    // answered by nobody and would condemn the *new* connection for the old
    // one's silence.
    awaiting = null;
  }

  /// Ask whether this connection is alive, and judge the previous answer.
  function ping() {
    if (socket?.readyState !== WebSocket.OPEN) return;
    if (awaiting && Date.now() - awaiting.sentAt > PONG_DEADLINE_MS) {
      // Asked, and nothing came back in time. The socket still claims to be
      // open and it is not; every edit made from here goes nowhere and no other
      // signal will ever say so.
      status("stale");
      awaiting = null;
      reconnect();
      return;
    }
    // One outstanding at a time. Piling them up would mean the deadline above
    // measured the oldest rather than the most recent, and a connection that
    // has just started failing would take several intervals to be noticed.
    if (awaiting) return;
    nonce += 1;
    awaiting = { nonce, sentAt: Date.now() };
    send({ type: "ping", nonce });
  }

  function flush() {
    if (!joined || socket?.readyState !== WebSocket.OPEN) return;
    if (mustResend) {
      mustResend = false;
      // Each with the *same* sequence number as before. That is what lets the
      // server recognise a chunk it already committed and answer with where it
      // landed, rather than applying it a second time — which, for an
      // insert-rows, is corruption rather than a glitch.
      //
      // In order, and all of them before anything new: only the first names a
      // revision and the rest chain from it, so one delivered early asks the
      // server to resolve a chain whose start it has not seen.
      for (const again of JSON.parse(wasm.collab_resend())) {
        socket.send(JSON.stringify(again));
      }
    }
    // One chunk, because a flush drains everything pending into one. What
    // changed with ADR-016 is that this is no longer *blocked* by chunks
    // already in flight: the next tick sends the next chunk rather than waiting
    // out a round trip first. Empty still means nothing to send — or that the
    // engine has hit its bound on outstanding chunks and is holding the rest.
    const submission = wasm.collab_flush();
    if (submission) socket.send(submission);
  }

  function send(message) {
    if (socket?.readyState === WebSocket.OPEN) socket.send(JSON.stringify(message));
  }

  /// Send local presence: where this participant is looking, and what they are
  /// part-way through typing.
  ///
  /// Carries no identity — the server takes that from the token, because
  /// presence is the one surface where a claimed name would be believed.
  ///
  /// `editing` is `{ at: [row, col], text }` while a cell editor is open and
  /// null the rest of the time, and passing null is how an **abandoned** edit is
  /// cleared everywhere: a participant owns exactly one presence entry and
  /// replaces it wholesale, so there is no separate "stop editing" message for a
  /// dropped socket to lose. A disconnect clears it the same way, by the entry
  /// going with its author.
  ///
  /// The message is built by the engine rather than here. It is the protocol,
  /// and the last time this file assembled one of its messages by hand the
  /// server could not parse a single edit any browser made — so the shape, and
  /// the bound on how much text a draft carries, live in one place that both
  /// ends compile from.
  function present(sheet, selection, editing = null) {
    if (socket?.readyState !== WebSocket.OPEN) return;
    const at = editing?.at ?? [0, 0];
    socket.send(
      wasm.collab_presence(sheet, Uint32Array.from(selection), at[0], at[1], editing?.text ?? undefined),
    );
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

  /// Follow a node's suggestion of somewhere with room. True if it was taken.
  function redirect(elsewhere) {
    if (redirected || closed) return false;
    const target = redirectTarget(endpoint, elsewhere);
    if (!target) return false;

    redirected = true;
    endpoint = target;
    status("connecting", "this server is full — trying another");
    // The old socket is finished with; the engine's session is not started
    // yet, so there is nothing to tear down beyond the transport itself.
    stopTimers();
    joined = false;
    socket?.close();
    socket = null;
    open();
    return true;
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

  /// The last measured round trip in milliseconds, or null before the first
  /// answer. What a host shows as a connection-quality indicator.
  function latency() {
    return roundTripMs;
  }

  open();
  return { present, close, flush, reconnect, latency, ping };
}

/// Where a `stopped` may send a client, or `null` if it may not (`DEP-09`).
///
/// The address comes from this deployment's own coordinator and is set by the
/// operator, so it is not attacker-supplied — but it is checked anyway, on the
/// principle that a value deciding where a *token* gets sent should never be
/// used unexamined:
///
/// - **A WebSocket URL**, so nothing else can be dialled. A `stopped` naming
///   `https://…` or `file:///…` is refused rather than followed.
/// - **No downgrade.** A session on `wss://` does not follow a redirect to
///   plain `ws://`. The browser would refuse it as mixed content anyway; being
///   refused here says why, instead of failing later with nothing to read.
///
/// Exported because these are the rules worth testing, and they are the part
/// with nothing to do with sockets. The "only once" bound lives with the
/// connection state that owns it.
export function redirectTarget(current, elsewhere) {
  if (!elsewhere) return null;
  let target;
  try {
    target = new URL(elsewhere, current);
  } catch {
    return null;
  }
  if (target.protocol !== "ws:" && target.protocol !== "wss:") return null;
  try {
    if (new URL(current).protocol === "wss:" && target.protocol !== "wss:") return null;
  } catch {
    return null;
  }
  return target.href;
}
