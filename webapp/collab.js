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
export function collaborate({
  url,
  token,
  document: documentKey,
  wasm,
  onStatus,
  onDocument,
  onPresence,
  /// How long a batch of relayed edits may spend recalculating, in ms.
  ///
  /// **The engine's convention, not a new one**: a negative or non-finite
  /// budget means no limit, and `0` is a real budget of no time rather than a
  /// way of saying "unbounded". Guarding on `> 0` here quietly turned a
  /// zero-budget test into an unbounded one, which is precisely the reading
  /// that made this defect untestable in the first place.
  ///
  /// The default is no limit, which is what this did for its whole life: a
  /// peer's edit that triggered an expensive pass held the tab, and nothing the
  /// person in front of it could do would stop that (`COL-43`).
  recalcBudgetMs = -1,
}) {
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

  /// Whether the rejoin for a desync has already been started on this socket.
  ///
  /// The engine latches on the first unmergeable arrival and answers `Desynced`
  /// for **every** later one (`COL-55`), so a burst of relayed edits throws once
  /// per message. Without this, each throw would rotate the key and close the
  /// socket again, and the client would thrash instead of rejoining. Cleared in
  /// `onclose`, so a second desync after a successful rejoin is answered the
  /// same way — under the ordinary backoff, which is what stops a document that
  /// desyncs every time from spinning.
  let rejoining = false;
  /// Set when the join now being made is the recovery from a desync, so the
  /// `welcome` that answers it can say *why* the unacknowledged work is gone.
  /// Cleared by that welcome.
  let desynced = false;

  // This editor's claim to be the participant it was, for reconnecting.
  //
  // Generated once and held for the life of this session — deliberately *not*
  // derived from the user, because the same person with the same token in two
  // tabs is two participants, and sharing an id would have each tab's
  // submissions suppress the other's as duplicates.
  //
  // Not a secret and not treated as one: the token is what authorises, and the
  // server only honours a key for the user it issued it to. See ADR-015.
  //
  // **Rotated in exactly one place**, `rejoin()`, and that is the whole
  // recovery from a desync: a key the server does not recognise is an ordinary
  // join, and an ordinary join comes back with a snapshot.
  let resumeKey = newKey();

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
      // **A throw in `receive` must not be allowed to end the session in
      // silence.** It had no handler, so an op the engine could not transform
      // — the `COL-44` refusal is the ordinary way to get one — escaped here,
      // and the socket stayed open while nothing was ever applied again. The
      // document quietly stopped being shared and looked completely fine:
      // no error, no status change, the peer's cursor still moving.
      //
      // Reported, not swallowed. This is the one failure where carrying on
      // silently is worse than stopping, because the user goes on typing into
      // a document they believe is shared.
      //
      // **And acted on** (`COL-56`). Reporting was only ever half of it: the
      // engine has latched, so this transport is now attached to a session that
      // refuses every arrival and sends nothing — a client in name only. The
      // status was the sole trace of that, and this editor has no line for
      // `desynced` at all, so the measured symptom was a status bar still
      // reading "collaborating" over a document that had stopped being shared.
      // `rejoin()` is the recovery the engine was designed around.
      try {
        receive(message);
      } catch (err) {
        status("desynced", String(err && err.message ? err.message : err));
        rejoin();
      }
    };

    socket.onclose = () => {
      stopTimers();
      joined = false;
      // Per socket, not per session: the guard exists to collapse a burst of
      // throws into one rejoin, and this socket is already gone.
      rejoining = false;
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
        // **Two different events reach this line, and a user told the wrong one
        // is a user misinformed rather than informed.** An unresumed reconnect
        // is a server that forgot us — a rolling deploy, a rebalance, an
        // evicted document. A desync is this client abandoning a state it can
        // no longer defend (`COL-55`). The cost is identical and the cause is
        // not, so it is named.
        //
        // Nothing here can save the work either way, and the reason is the same
        // in both: those operations were written against a revision this client
        // no longer holds, and re-applying them untransformed is exactly the
        // divergence the transform exists to prevent. What is offered is that
        // the loss is **named before the snapshot lands**, which is why this
        // runs first — afterwards there is nothing left to ask about.
        //
        // The status detail keeps answering **what** went — that is the
        // question a status line answers, and it is the same answer either way
        // — and the cause rides on the document event, where a host that wants
        // to word it differently can read it.
        const why = desynced ? "desynced" : "unresumed";
        if (losing) {
          lostUnsent = true;
          status("lost", "unsentEdits");
          onDocument({ reason: "lost", why, revision: message.revision });
        }
        // The snapshot is the document as everyone else has it *at this
        // revision*. Loading the file instead would start this participant at
        // revision zero while the session was at five hundred.
        //
        // `Uint8Array`, not the array JSON gave us: a `Vec<u8>` crosses as an
        // array of numbers, and the binding takes a byte slice. Handing it the
        // plain array throws at the boundary rather than converting.
        wasm.session_load_snapshot(new Uint8Array(message.snapshot));
        // `collab_begin`, not `collab_resume`, is what clears a latched
        // session: it replaces the `ClientSession` outright, and
        // `session_load_snapshot` above replaced the workbook and its applied
        // log with it. So the desync is over exactly here, and only here — a
        // resume deliberately keeps the latch, which is why `rejoin()` has to
        // make the server *unable* to resume us.
        wasm.collab_begin(message.client, message.revision);
        desynced = false;
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
        const missedLeftStale = applyRelayed(message.missed, message.revision);
        revision = message.revision;
        joined = true;
        // Now, and not before: the chunk in flight had to be rebased past what
        // arrived above, or it would be submitted against a revision the server
        // has moved beyond.
        mustResend = true;
        retryMs = RETRY_FLOOR_MS;
        // Reported as a remote change, because that is what it is: the missed
        // operations are in the document and only their values are behind.
        if (missedLeftStale) {
          onDocument({ reason: "remote", revision, stale: true });
        }
        onDocument({ reason: "resumed", revision, editable: message.editable });
        // **Held while something is still lost**, exactly as the `welcome`
        // above holds it, and for the reason `lostUnsent` exists at all: "live"
        // is what erases the notice, and a notice erased by the *next* socket
        // event is a notice nobody reads. This branch said it unconditionally,
        // so a reconnect that resumed — the ordinary case, since a rotated key
        // is recognised the moment the rejoin succeeds — wiped the loss notice
        // with nothing having been acknowledged. Measured: the status line went
        // from naming the lost edits back to "collaborating" on the next
        // reconnect, which is the same disappearing-notice failure `COL-56` is
        // about, one socket later. `ack` is the only thing that clears it, and
        // it clears it because work made *since* the loss has landed.
        if (!lostUnsent) status("live");
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
      case "apply": {
        const leftStale = applyRelayed(message.ops, message.revision);
        revision = message.revision;
        onDocument({
          reason: "remote",
          revision: message.revision,
          // The edits are in the document either way; these are the values
          // that did not finish following from them.
          stale: leftStale,
        });
        break;
      }
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

  /// Apply relayed operations, bounded as a batch.
  ///
  /// **One budget for the whole batch, not one per operation.** A burst of
  /// twenty relayed edits with a 50 ms budget each is a second of frozen tab,
  /// which is the thing being prevented rather than a smaller version of it.
  ///
  /// Every operation is applied whichever way the recalculation goes: the model
  /// converges on cell content, and only derived values are left behind. True
  /// if anything was.
  function applyRelayed(ops, atRevision) {
    wasm.session_set_time_budget_ms(recalcBudgetMs);
    let stale = false;
    try {
      for (const op of ops) {
        const outcome = wasm.collab_receive(JSON.stringify(op), atRevision);
        if (outcome && outcome !== "full" && outcome !== "none") stale = true;
      }
    } finally {
      wasm.session_clear_time_budget();
    }
    return stale;
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

  /// Stop being this participant, and come back as a new one (`COL-56`).
  ///
  /// The recovery from a desync, and it is deliberately an **ordinary** one:
  /// close the socket, drop the resume key, reconnect, take the `welcome`
  /// snapshot. No message, no field and no enum variant crosses the wire for
  /// it, so the server needs to know nothing it does not already.
  ///
  /// **A plain `reconnect()` is not this, and does not work.** Measured: the
  /// socket comes back, the server recognises the key and answers `resumed`,
  /// and `collab_resume` deliberately *keeps* the engine's latch — a resume
  /// asserts continuity, which is the one thing that is false here. The very
  /// next relayed operation throws `Desynced` again. So the key has to go: an
  /// unrecognised one is an ordinary join, and an ordinary join is answered
  /// with a snapshot.
  ///
  /// **What it costs, and why nothing is offered instead of a warning.**
  /// Everything unacknowledged — `collab_unacknowledged()` still answers, and
  /// the `welcome` above asks it before the snapshot lands, so the loss is
  /// named rather than silent. It cannot be kept: those edits were written
  /// against a revision this client has abandoned, and the document they were
  /// written against is itself missing a committed revision, so preserving
  /// either would be preserving a divergence rather than rescuing work.
  function rejoin() {
    if (closed || rejoining) return;
    rejoining = true;
    desynced = true;
    resumeKey = newKey();
    // Zero rather than where this client thought it was. The key is new, so
    // the revision is not consulted — but a client that has abandoned its
    // history has no business claiming a place in one.
    revision = 0;
    joined = false;
    // The outstanding chunks are sealed by the engine and are not coming back;
    // a resend flag surviving into the new session would ask it to send work
    // the old one owned.
    mustResend = false;
    stopTimers();
    // Through `onclose`, which is the same path a real drop takes — and which
    // applies the ordinary backoff, so a document that desyncs on every join
    // slows down instead of hammering the server.
    socket?.close();
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
