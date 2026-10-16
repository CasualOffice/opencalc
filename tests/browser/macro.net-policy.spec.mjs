// Which addresses a macro or custom function may even ask for (`MACRO-13`,
// docs/99 §5a). Pure policy, run in Node against the same module the editor
// uses — `new URL` here is the WHATWG parser `fetch` uses, so `0x7f.1` is read
// as the browser would read it.

import { expect, test } from "@playwright/test";
import { NET_LIMITS, checkRequest, checkUrl } from "../../webapp/macro.net-policy.js";

const PAGE = "https://opencalc.example.org";
const COLLAB = ["https://collab.example.net"];

/// Every one of these must be refused, before anybody is asked.
const HOSTILE = [
  // Scheme.
  ["http://api.example.com/", "only https"],
  ["ws://api.example.com/", "only https"],
  ["wss://api.example.com/", "only https"],
  ["ftp://api.example.com/", "only https"],
  ["file:///etc/passwd", "only https"],
  ["data:text/plain,hi", "only https"],
  ["blob:https://api.example.com/x", "only https"],
  ["javascript:alert(1)", "only https"],
  ["//api.example.com/", "not an absolute"],
  ["/relative", "not an absolute"],
  ["", "not an absolute"],
  // Credentials and ports.
  ["https://user@api.example.com/", "user name"],
  ["https://user:pw@api.example.com/", "user name"],
  ["https://:pw@api.example.com/", "user name"],
  ["https://api.example.com:8443/", "default port"],
  ["https://api.example.com:80/", "default port"],
  ["https://api.example.com:1/", "default port"],
  // IPv4, every spelling the parser accepts.
  ["https://127.0.0.1/", "IP addresses"],
  ["https://10.0.0.1/", "IP addresses"],
  ["https://192.168.1.1/", "IP addresses"],
  ["https://169.254.169.254/latest/meta-data/", "IP addresses"],
  ["https://8.8.8.8/", "IP addresses"],
  ["https://0x7f.1/", "IP addresses"],
  ["https://0x7f000001/", "IP addresses"],
  ["https://2130706433/", "IP addresses"],
  ["https://017700000001/", "IP addresses"],
  ["https://127.1/", "IP addresses"],
  ["https://0/", "IP addresses"],
  ["https://0.0.0.0/", "IP addresses"],
  ["https://127.0.0.1./", "IP addresses"],
  ["https://%31%32%37.0.0.1/", "IP addresses"],
  // IPv6.
  ["https://[::1]/", "IP addresses"],
  ["https://[::]/", "IP addresses"],
  ["https://[::ffff:127.0.0.1]/", "IP addresses"],
  ["https://[::ffff:7f00:1]/", "IP addresses"],
  ["https://[fe80::1]/", "IP addresses"],
  ["https://[fd00::1]/", "IP addresses"],
  ["https://[2001:4860:4860::8888]/", "IP addresses"],
  // Single-label and local names.
  ["https://intranet/", "single-label"],
  ["https://localhost/", "single-label"],
  ["https://LOCALHOST/", "single-label"],
  ["https://localhost./", "single-label"],
  ["https://router/", "single-label"],
  ["https://foo.localhost/", "local-network"],
  ["https://printer.local/", "local-network"],
  ["https://db.internal/", "local-network"],
  ["https://nas.lan/", "local-network"],
  ["https://nas.LAN./", "local-network"],
  ["https://hub.home.arpa/", "local-network"],
  ["https://site.test/", "local-network"],
  // Our own servers.
  ["https://opencalc.example.org/api", "own servers"],
  ["https://OpenCalc.Example.org./x", "own servers"],
  ["https://collab.example.net/collab", "own servers"],
];

for (const [url, why] of HOSTILE) {
  test(`refused: ${url || "(empty)"}`, () => {
    const r = checkUrl(url, { pageOrigin: PAGE, collabOrigins: COLLAB, allowlist: null });
    expect(r.ok, `${url} was allowed`).toBe(false);
    expect(r.reason).toContain(why);
  });
}

test("a public https site on the default port is allowed, and named by scheme and host only", () => {
  const r = checkUrl("https://API.Example.com./v1/rates?base=EUR#x", { pageOrigin: PAGE, collabOrigins: COLLAB });
  expect(r).toEqual({ ok: true, url: "https://api.example.com/v1/rates?base=EUR#x", site: "https://api.example.com" });
  // The explicit default port is the default port.
  expect(checkUrl("https://api.example.com:443/", { pageOrigin: PAGE }).ok).toBe(true);
  // A name that only looks numeric in part is a name.
  expect(checkUrl("https://1.example.com/", { pageOrigin: PAGE }).ok).toBe(true);
});

test("the host's allowlist admits only the sites it names", () => {
  const allowlist = ["api.example.com", "https://Data.Example.org", "http://insecure.example.com", "not a site"];
  const at = (u) => checkUrl(u, { pageOrigin: PAGE, allowlist });
  expect(at("https://api.example.com/x").ok).toBe(true);
  expect(at("https://data.example.org/x").ok).toBe(true);
  expect(at("https://evil.example.com/").reason).toContain("host's list");
  // Not a subdomain, and not by a suffix.
  expect(at("https://x.api.example.com/").ok).toBe(false);
  expect(at("https://api.example.com.evil.net/").ok).toBe(false);
  // An http entry names no https site.
  expect(at("https://insecure.example.com/").ok).toBe(false);
  // An empty list allows nothing.
  expect(checkUrl("https://api.example.com/", { pageOrigin: PAGE, allowlist: [] }).ok).toBe(false);
  // The list never re-admits what the address rules refuse.
  expect(checkUrl("https://localhost/", { pageOrigin: PAGE, allowlist: ["localhost"] }).ok).toBe(false);
  expect(checkUrl("https://127.0.0.1/", { pageOrigin: PAGE, allowlist: ["127.0.0.1"] }).ok).toBe(false);
});

test("a request is cut down to GET, HEAD or POST, Accept and Content-Type, and 1 MB", () => {
  expect(checkRequest({})).toEqual({ ok: true, method: "GET", headers: {}, body: null });
  expect(checkRequest({ method: "post", headers: { "Content-Type": "application/json", Accept: "application/json" }, body: "{}" }))
    .toEqual({ ok: true, method: "POST", headers: { "content-type": "application/json", accept: "application/json" }, body: "{}" });
  for (const method of ["PUT", "DELETE", "PATCH", "OPTIONS", "CONNECT", "TRACE"]) {
    expect(checkRequest({ method }).ok, method).toBe(false);
  }
  for (const h of ["Authorization", "Cookie", "X-Requested-With", "Origin", "Referer", "Host", "X-Forwarded-For"]) {
    expect(checkRequest({ headers: { [h]: "x" } }).ok, h).toBe(false);
  }
  expect(checkRequest({ headers: { Accept: "a\r\nCookie: x" } }).ok).toBe(false);
  expect(checkRequest({ method: "GET", body: "x" }).ok).toBe(false);
  expect(checkRequest({ method: "POST", body: { notString: true } }).ok).toBe(false);
  expect(checkRequest({ method: "POST", body: "x".repeat(NET_LIMITS.maxRequestBytes) }).ok).toBe(true);
  expect(checkRequest({ method: "POST", body: "x".repeat(NET_LIMITS.maxRequestBytes + 1) }).ok).toBe(false);
  // Counted in bytes, not characters.
  expect(checkRequest({ method: "POST", body: "é".repeat(NET_LIMITS.maxRequestBytes / 2 + 1) }).ok).toBe(false);
});
