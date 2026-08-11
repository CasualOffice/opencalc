// Minting a token the way an integrator would.
//
// The collaboration server verifies tokens and deliberately cannot mint them,
// which is the right division and leaves a test needing the other half. This is
// that half, in thirty lines, because HS256 is an HMAC over two base64url
// segments and pulling in a JWT library to do it would hide the one thing worth
// seeing: the claims are exactly the integration contract, and nothing else
// passes between a host and this server.
//
// HS256 rather than RS256 on purpose. A shared secret is what a test can set up
// in an environment variable, and the server warns loudly when one is used —
// the asymmetric path is what a deployment should use and needs a key pair and
// a JWKS endpoint to exercise, which is a different test.

import { createHmac } from "node:crypto";

const encode = (value) =>
  Buffer.from(typeof value === "string" ? value : JSON.stringify(value)).toString("base64url");

/// Sign `claims` with `secret`.
export function mint(claims, secret) {
  // No `kid`. The shared-secret key set has exactly one key, and naming it
  // would test the selection path rather than this one.
  const header = encode({ alg: "HS256", typ: "JWT" });
  const body = encode(claims);
  const signature = createHmac("sha256", secret).update(`${header}.${body}`).digest("base64url");
  return `${header}.${body}.${signature}`;
}

/// A token for `document`, as `user`, valid for the next ten minutes.
///
/// `access` is the permission level: "view", "comment" or "edit". It is a
/// parameter rather than a constant because a read-only participant is the case
/// most likely to be got wrong — a client that hides a toolbar and a server that
/// enforces nothing look identical until someone edits anyway.
export function tokenFor({ document, user, origin, access = "edit", audience = "opencalc-test" }) {
  const now = Math.floor(Date.now() / 1000);
  return {
    claims: {
      iss: "opencalc-browser-tests",
      aud: audience,
      exp: now + 600,
      iat: now,
      user: { id: user.id, name: user.name, color: user.color },
      document: {
        key: document,
        id: `file-${document}`,
        title: "minimal.xlsx",
        // Where the server fetches the original package. A session starts from
        // the file, never from a snapshot.
        url: `${origin}/minimal.xlsx`,
      },
      permissions: { access },
      // No callback: this test is about the conversation, and a server that
      // tried to post the result somewhere would need somewhere to post it.
      // Absent means the server will not save, which it says out loud.
    },
  };
}
