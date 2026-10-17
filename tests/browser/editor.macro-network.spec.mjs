// Network access for macros and custom functions, only after the person
// allows it (`MACRO-13`, docs/99 §5a).
//
// The fake sites are Playwright routes: nothing leaves the machine, and every
// request the page makes is seen here — its method, its headers, whether it
// carried a cookie or a referrer, and whether a redirect was followed.

import zlib from "node:zlib";
import { expect, test } from "@playwright/test";

const API = "https://api.example.com";

async function boot(page) {
  await page.goto("/editor.html");
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await page.evaluate(() => {
    window.opencalcEditor.wasmApi().session_new();
    window.opencalcEditor.forgetMacroAnswersForTest?.();
    window.opencalcEditor.selectForTest(0, 0);
  });
  await page.locator("#grid").focus();
}

/// A fake `https://api.example.com`. `reply(route, request)` may answer; the
/// default is `{"rate":1.5}` with the CORS header a public API sends.
async function fakeApi(page, reply) {
  const seen = [];
  await page.route(`${API}/**`, async (route) => {
    const r = route.request();
    seen.push({ url: r.url(), method: r.method(), headers: await r.allHeaders(), body: r.postData() });
    if (reply) return reply(route, r);
    return route.fulfill({
      status: 200,
      headers: { "access-control-allow-origin": "*", "content-type": "application/json", "x-secret": "not for scripts" },
      body: JSON.stringify({ rate: 1.5 }),
    });
  });
  return seen;
}

const notice = (page) => page.locator("#oc-net-notice");
const bar = (page) => page.locator("#oc-net-bar");
const input = (page, r, c) => page.evaluate(([r, c]) => window.opencalcEditor.wasmApi().session_cell_input(0, r, c), [r, c]);
const shown = (page, r, c) =>
  page.evaluate(([r, c]) => JSON.parse(window.opencalcEditor.wasmApi().session_cells(0, r, c, r, c))[0]?.t ?? "", [r, c]);

/// Start a macro; the promise settles when the run ends (after any prompt).
function startMacro(page, source, name = "Fetch rates") {
  return page.evaluate(([s, n]) => window.opencalcEditor.runMacroForTest(s, { name: n }), [source, name]);
}

async function typeAt(page, r, c, text) {
  await page.evaluate(([r, c]) => window.opencalcEditor.selectForTest(r, c), [r, c]);
  await page.locator("#grid").focus();
  await page.keyboard.type(text);
  await page.keyboard.press("Enter");
}

async function defineFunctions(page, source) {
  await page.locator("#grid").focus();
  await page.keyboard.press("Alt+F8");
  await page.locator("#oc-mw-new-function").click();
  await page.locator("#oc-mw-code").fill(source);
  await page.locator("#oc-mw-run").click();
  await expect(page.locator("#oc-mw-result")).toHaveText(/Registered|Line|Error|error/, { timeout: 20_000 });
  await page.locator("#oc-mw-close").click();
}

const FXRATE = [
  "/**",
  " * Today's rate.",
  " * @customfunction",
  " * @returns {any} The rate.",
  " */",
  "async function FXRATE() {",
  `  const r = await fetch("${API}/v1/rate?base=EUR", { headers: { Accept: "application/json" } });`,
  "  return (await r.json()).rate;",
  "}",
  "Api.AddCustomFunction(FXRATE);",
].join("\n");

/// A macro that fetches and writes what it got into A1.
const MACRO = `(async function () {
  var r = await fetch("${API}/v1/rate");
  var j = await r.json();
  Api.GetActiveSheet().GetRange("A1").SetValue(j.rate);
})();`;

// ---------------------------------------------------------------------------
// 1. The sandbox holds no network primitive.

/// Every way a script might reach a real `fetch`, `XMLHttpRequest`,
/// `WebSocket`, `EventSource`, `importScripts` or `sendBeacon`. Each probe
/// answers "refused", "absent", "stub" or "NATIVE …" — the last is a hole.
const PROBES = String.raw`
  var out = {};
  var native = function (f) {
    try { return typeof f === "function" && /\[native code\]/.test(Object.getPrototypeOf(function () {}).toString.call(f)); }
    catch (e) { return false; }
  };
  var NAMES = ["fetch", "XMLHttpRequest", "WebSocket", "EventSource", "importScripts", "sendBeacon", "WebTransport", "Request", "Worker", "SharedWorker"];
  var probe = function (label, get) {
    try {
      var v = get();
      if (v === undefined || v === null) out[label] = "absent";
      else if (v === fetch) out[label] = "stub";
      else if (native(v)) out[label] = "NATIVE " + (v.name || "?");
      else out[label] = typeof v;
    } catch (e) { out[label] = "refused"; }
  };
  var g = (function () { return this; })() || globalThis;
  NAMES.forEach(function (n) {
    probe("self." + n, function () { return self[n]; });
    probe("globalThis." + n, function () { return globalThis[n]; });
    probe("sloppy this." + n, function () { return g[n]; });
    /* Every object on the global's prototype chain, by descriptor. */
    for (var p = globalThis, depth = 0; p; p = Object.getPrototypeOf(p), depth++) {
      (function (p, depth) {
        probe("proto" + depth + "." + n + " value", function () { var d = Object.getOwnPropertyDescriptor(p, n); return d && d.value; });
        probe("proto" + depth + "." + n + " getter", function () { var d = Object.getOwnPropertyDescriptor(p, n); return d && d.get && d.get.call(globalThis); });
      })(p, depth);
    }
    probe("constructor.prototype." + n, function () { return globalThis.constructor.prototype[n]; });
    probe("Reflect.get " + n, function () { return Reflect.get(globalThis, n); });
  });
  probe("Function('return this')", function () { return Object.getPrototypeOf(function () {}).constructor("return fetch")(); });
  probe("fetch.constructor", function () { return fetch.constructor("return self.fetch")(); });
  probe("[].constructor.constructor", function () { return [].constructor.constructor("return self.fetch")(); });
  probe("async constructor", function () { return (async function () {}).constructor("return self.fetch"); });
  probe("generator constructor", function () { return (function* () {}).constructor("return self.fetch"); });
  probe("navigator.sendBeacon", function () { return navigator.sendBeacon; });
  probe("eval", function () { return eval("self.fetch"); });
  probe("setTimeout string", function () { setTimeout("self.__x = self.fetch", 0); return self.__x; });
  probe("fetch is the stub, not native", function () { return native(fetch) ? fetch : undefined; });
  /* Own names of every object on the chain: any native networking function. */
  for (var p2 = globalThis, d2 = 0; p2; p2 = Object.getPrototypeOf(p2), d2++) {
    Object.getOwnPropertyNames(p2).forEach(function (k) {
      var d = Object.getOwnPropertyDescriptor(p2, k);
      if (d && typeof d.value === "function" && NAMES.indexOf(d.value.name) >= 0 && native(d.value)) out["own " + d2 + "." + k] = "NATIVE " + d.value.name;
    });
  }
`;

test("a macro cannot recover a real network primitive, by any route", async ({ page }) => {
  await boot(page);
  const seen = await fakeApi(page);
  const res = await startMacro(page, `${PROBES}\nApi.GetActiveSheet().GetRange("A1").SetValue(JSON.stringify(out));`);
  expect(res.error).toBeNull();
  const out = JSON.parse(await input(page, 0, 0));
  const holes = Object.entries(out).filter(([, v]) => /^NATIVE/.test(v));
  expect(holes, `native primitives reachable: ${JSON.stringify(holes)}`).toEqual([]);
  expect(Object.keys(out).length).toBeGreaterThan(50);
  expect(seen).toEqual([]);
});

test("a custom function cannot recover a real network primitive, by any route", async ({ page }) => {
  await boot(page);
  await defineFunctions(page, [
    "/**",
    " * @customfunction",
    " * @returns {string}",
    " */",
    `function PROBE() { ${PROBES.replace(/\n/g, " ")} return JSON.stringify(out); }`,
    "Api.AddCustomFunction(PROBE);",
  ].join("\n"));
  await typeAt(page, 0, 0, "=PROBE()");
  await expect.poll(() => shown(page, 0, 0), { timeout: 10_000 }).toMatch(/^\{/);
  const out = JSON.parse(await shown(page, 0, 0));
  const holes = Object.entries(out).filter(([, v]) => /^NATIVE/.test(v));
  expect(holes, `native primitives reachable: ${JSON.stringify(holes)}`).toEqual([]);
  expect(Object.keys(out).length).toBeGreaterThan(50);
});

// ---------------------------------------------------------------------------
// 2. Consent: Excel's Security Notice and Security Warning bar.

test("a custom function's first request waits at #BUSY! for the Security Notice; Enable answers it", async ({ page }) => {
  await boot(page);
  const seen = await fakeApi(page);
  await defineFunctions(page, FXRATE);
  await typeAt(page, 0, 0, "=FXRATE()");
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  expect(await shown(page, 0, 0)).toBe("#BUSY!");
  expect(seen, "nothing is sent before Enable").toEqual([]);
  // Excel's notice: title, the concern, then Enable and Disable in that order.
  await expect(page.locator("#oc-net-notice-title")).toHaveText("OpenCalc Security Notice");
  await expect(notice(page)).toContainText("OpenCalc has identified a potential security concern.");
  const text = await page.locator("#oc-net-notice-text").textContent();
  expect(text).toContain("The custom function FXRATE wants to connect to https://api.example.com,");
  expect(text).toContain("data from this workbook can be sent to it");
  // Scheme and host only: not the path or the query.
  expect(text).not.toContain("/v1/rate");
  expect(text).not.toContain("base=EUR");
  expect(await notice(page).locator(".oc-confirm-actions button").allTextContents()).toEqual(["Enable", "Disable"]);
  // The safe answer has the focus.
  await expect(page.locator("#oc-net-notice-disable")).toBeFocused();
  // The bar says the same.
  await expect(bar(page)).toBeVisible();
  await expect(bar(page)).toContainText("SECURITY WARNING");
  await expect(page.locator("#oc-net-bar-enable")).toHaveText("Enable Content");
  // Still waiting, well past the 2 s a call may run: the person's time is
  // not the function's.
  await page.waitForTimeout(2_500);
  expect(await shown(page, 0, 0)).toBe("#BUSY!");

  await page.locator("#oc-net-notice-enable").click();
  await expect.poll(() => shown(page, 0, 0), { timeout: 10_000 }).toBe("1.5");
  await expect(notice(page)).toBeHidden();
  await expect(bar(page)).toBeHidden();
  expect(seen).toHaveLength(1);
});

test("Disable rejects with NetworkBlocked: an unhandled call is #VALUE!, and nothing is sent", async ({ page }) => {
  await boot(page);
  const seen = await fakeApi(page);
  // Two functions in one source: one grant covers them both.
  await defineFunctions(page, [
    FXRATE,
    "/**",
    " * @customfunction",
    " * @returns {string}",
    " */",
    `async function WHY() { try { await fetch("${API}/x"); return "sent"; } catch (e) { return e.name; } }`,
    "Api.AddCustomFunction(WHY);",
  ].join("\n"));
  await typeAt(page, 0, 0, "=FXRATE()");
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  await page.locator("#oc-net-notice-disable").click();
  await expect.poll(() => shown(page, 0, 0), { timeout: 10_000 }).toBe("#VALUE!");
  expect(seen).toEqual([]);
  // Excel keeps the bar up after Disable, with Enable Content.
  await expect(bar(page)).toBeVisible();
  // Disabled stays disabled in this tab, without asking again; a handled
  // rejection names the error.
  await typeAt(page, 1, 0, "=WHY()");
  await expect.poll(() => shown(page, 1, 0), { timeout: 10_000 }).toBe("NetworkBlocked");
  await expect(notice(page)).toBeHidden();
  expect(seen).toEqual([]);
});

test("Enable Content recomputes a custom function that Disable left at #VALUE! (MACRO-19)", async ({ page }) => {
  // Excel's Enable Content refreshes what the blocked content feeds; leaving
  // the cell at #VALUE! until a manual recalculation looked like a broken
  // function, not like content that is now allowed.
  await boot(page);
  const seen = await fakeApi(page);
  await defineFunctions(page, FXRATE);
  await typeAt(page, 0, 0, "=FXRATE()");
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  await page.locator("#oc-net-notice-disable").click();
  await expect.poll(() => shown(page, 0, 0), { timeout: 10_000 }).toBe("#VALUE!");
  expect(seen).toEqual([]);
  await bar(page).getByRole("button", { name: "Enable Content" }).click();
  await expect.poll(() => shown(page, 0, 0), { timeout: 10_000 }).toBe("1.5");
  expect(seen.length).toBe(1);
});

test("a macro's requests to one site are one prompt; Disable fails the run and changes nothing", async ({ page }) => {
  await boot(page);
  const seen = await fakeApi(page);
  const run = startMacro(page, `(async function () {
    var all = await Promise.all([fetch("${API}/a"), fetch("${API}/b"), fetch("${API}/c")]);
    Api.GetActiveSheet().GetRange("A1").SetValue(all.length);
  })();`);
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  await expect(page.locator("#oc-net-notice-text")).toContainText('The macro "Fetch rates" wants to connect to https://api.example.com');
  await page.locator("#oc-net-notice-disable").click();
  // One prompt for the three.
  await page.waitForTimeout(300);
  await expect(notice(page)).toBeHidden();
  const res = await run;
  expect(res.ok).toBe(false);
  expect(res.error.name).toBe("NetworkBlocked");
  expect(res.operations).toEqual([]);
  expect(await input(page, 0, 0)).toBe("");
  expect(seen).toEqual([]);
});

test("a macro allowed once runs; the same macro is not asked again; editing it asks again", async ({ page }) => {
  await boot(page);
  const seen = await fakeApi(page);
  let run = startMacro(page, MACRO);
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  await page.locator("#oc-net-notice-enable").click();
  let res = await run;
  expect(res.error).toBeNull();
  expect(await input(page, 0, 0)).toBe("1.5");
  expect(seen).toHaveLength(1);

  res = await startMacro(page, MACRO);
  expect(res.error).toBeNull();
  expect(seen).toHaveLength(2);

  // One character different is a different program.
  run = startMacro(page, MACRO.replace("v1/rate", "v1/rate?x=1"));
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  await page.locator("#oc-net-notice-disable").click();
  res = await run;
  expect(res.error.name).toBe("NetworkBlocked");
  expect(seen).toHaveLength(2);
});

// ---------------------------------------------------------------------------
// 3. The request is ours, not the script's.

test("the request carries no cookie, no referrer, no credentials and only Accept and Content-Type", async ({ page, context }) => {
  await context.addCookies([{ name: "session", value: "secret", domain: "api.example.com", path: "/", secure: true, sameSite: "None" }]);
  await boot(page);
  const seen = await fakeApi(page);
  const run = startMacro(page, `(async function () {
    var r = await fetch("${API}/v1/post", {
      method: "POST",
      headers: { "Content-Type": "text/plain", Accept: "text/plain" },
      body: "hello",
      credentials: "include", mode: "no-cors", redirect: "follow", referrerPolicy: "unsafe-url", cache: "force-cache",
    });
    var keys = Object.keys(r).sort().join(",");
    Api.GetActiveSheet().GetRange("A1").SetValue(r.status + "|" + r.ok + "|" + keys + "|" + typeof r.headers + "|" + typeof r.body + "|" + typeof r.url);
  })();`);
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  await page.locator("#oc-net-notice-enable").click();
  const res = await run;
  expect(res.error).toBeNull();
  // Only status, ok, text() and json(); no headers, body or URL.
  expect(await input(page, 0, 0)).toBe("200|true|json,ok,status,text|undefined|undefined|undefined");
  expect(seen).toHaveLength(1);
  const [req] = seen;
  expect(req.method).toBe("POST");
  expect(req.body).toBe("hello");
  expect(req.headers.cookie, "a cookie was sent").toBeUndefined();
  expect(req.headers.referer, "a referrer was sent").toBeUndefined();
  expect(req.headers.authorization).toBeUndefined();
  expect(req.headers["content-type"]).toBe("text/plain");
  expect(req.headers.accept).toBe("text/plain");
});

test("the page makes the request with docs/99's fixed options, whatever the script asked for", async ({ page }) => {
  // What the page hands the browser's own fetch. The network-level tests
  // above cannot see every option — Chromium does not follow a redirect that
  // a Playwright route fulfils for a CORS fetch, whatever \`redirect\` says —
  // so the options themselves are held here.
  await page.addInitScript(() => {
    const real = window.fetch.bind(window);
    window.__netCalls = [];
    window.fetch = (input, init) => {
      if (String(input).startsWith("https://api.example.com/")) {
        const { signal, ...rest } = init || {};
        window.__netCalls.push({ url: String(input), init: rest, signal: signal instanceof AbortSignal });
      }
      return real(input, init);
    };
  });
  await boot(page);
  await fakeApi(page);
  const run = startMacro(page, `(async function () {
    await fetch("${API}/v1/post", {
      method: "POST", headers: { "Content-Type": "text/plain" }, body: "hi",
      credentials: "include", mode: "no-cors", redirect: "follow", referrerPolicy: "unsafe-url", cache: "force-cache",
      keepalive: true, integrity: "sha256-x", priority: "high",
    });
  })();`);
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  await page.locator("#oc-net-notice-enable").click();
  expect((await run).error).toBeNull();
  const calls = await page.evaluate(() => window.__netCalls);
  expect(calls).toEqual([{
    url: "https://api.example.com/v1/post",
    init: {
      method: "POST",
      headers: { "content-type": "text/plain" },
      body: "hi",
      credentials: "omit",
      referrerPolicy: "no-referrer",
      cache: "no-store",
      mode: "cors",
      redirect: "error",
    },
    signal: true,
  }]);
});

test("headers, methods and bodies outside the rules are refused before anybody is asked", async ({ page }) => {
  await boot(page);
  const seen = await fakeApi(page);
  const res = await startMacro(page, `(async function () {
    var out = [];
    var tries = [
      [{ headers: { Authorization: "Bearer x" } }],
      [{ headers: { Cookie: "a=b" } }],
      [{ method: "PUT" }],
      [{ method: "DELETE" }],
      [{ method: "POST", body: { a: 1 } }],
      [{ method: "POST", body: "x".repeat(1024 * 1024 + 1) }],
    ];
    for (var i = 0; i < tries.length; i++) {
      try { await fetch("${API}/x", tries[i][0]); out.push("sent"); } catch (e) { out.push(e.name); }
    }
    Api.GetActiveSheet().GetRange("A1").SetValue(out.join(","));
  })();`);
  expect(res.error).toBeNull();
  expect(await input(page, 0, 0)).toBe("NetworkBlocked,NetworkBlocked,NetworkBlocked,NetworkBlocked,NetworkBlocked,NetworkBlocked");
  await expect(notice(page)).toBeHidden();
  expect(seen).toEqual([]);
});

test("hostile addresses are refused without a prompt", async ({ page }) => {
  await boot(page);
  const res = await startMacro(page, `(async function () {
    var urls = ["http://api.example.com/", "https://127.0.0.1/", "https://0x7f.1/", "https://2130706433/", "https://[::1]/",
      "https://[::ffff:127.0.0.1]/", "https://localhost/", "https://printer.local/", "https://intranet/", "https://u:p@api.example.com/",
      "https://api.example.com:8443/", location.origin + "/editor.html", "https://8.8.8.8/", "https://[2001:4860:4860::8888]/",
      "https://0x08080808/"];
    var out = [];
    for (var i = 0; i < urls.length; i++) {
      try { await fetch(urls[i]); out.push("sent"); } catch (e) { out.push(e.name); }
    }
    Api.GetActiveSheet().GetRange("A1").SetValue(out.join(","));
  })();`.replace("location.origin", JSON.stringify(new URL(page.url()).origin.replace(/^http:/, "https:"))));
  expect(res.error).toBeNull();
  expect((await input(page, 0, 0)).split(",")).toEqual(Array(15).fill("NetworkBlocked"));
  await expect(notice(page)).toBeHidden();
  await expect(bar(page)).toBeHidden();
});

test("a redirect is not followed: the request fails and the second site is never asked", async ({ page }) => {
  await boot(page);
  let followed = 0;
  await page.route("https://elsewhere.example.net/**", (route) => { followed++; return route.fulfill({ status: 200, body: "x", headers: { "access-control-allow-origin": "*" } }); });
  await fakeApi(page, (route) => route.fulfill({ status: 302, headers: { location: "https://elsewhere.example.net/steal", "access-control-allow-origin": "*" } }));
  const run = startMacro(page, `(async function () {
    try { await fetch("${API}/go"); Api.GetActiveSheet().GetRange("A1").SetValue("followed"); }
    catch (e) { Api.GetActiveSheet().GetRange("A1").SetValue(e.name); }
  })();`);
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  await page.locator("#oc-net-notice-enable").click();
  expect((await run).error).toBeNull();
  expect(await input(page, 0, 0)).toBe("NetworkError");
  expect(followed).toBe(0);
});

test("a reply over 5 MB is cut off; 5 MB is not", async ({ page }) => {
  await boot(page);
  const FIVE = 5 * 1024 * 1024;
  await fakeApi(page, (route, r) => route.fulfill({
    status: 200,
    headers: { "access-control-allow-origin": "*", "content-type": "text/plain" },
    body: "x".repeat(r.url().endsWith("/big") ? FIVE + 1 : FIVE),
  }));
  const run = startMacro(page, `(async function () {
    var out = [];
    try { var t = await (await fetch("${API}/exact")).text(); out.push(t.length); } catch (e) { out.push(e.name + ":" + e.message); }
    try { var t2 = await (await fetch("${API}/big")).text(); out.push(t2.length); } catch (e) { out.push(e.name + ":" + e.message); }
    Api.GetActiveSheet().GetRange("A1").SetValue(out.join("|"));
  })();`);
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  await page.locator("#oc-net-notice-enable").click();
  expect((await run).error).toBeNull();
  expect(await input(page, 0, 0)).toBe(`${FIVE}|NetworkError:NetworkError: the reply is larger than 5 MB`);
});

test("a site that does not answer is given up on at 10 s", async ({ page }) => {
  test.setTimeout(60_000);
  await boot(page);
  await fakeApi(page, () => new Promise(() => {}));
  const run = startMacro(page, `(async function () {
    var t0 = Date.now();
    try { await fetch("${API}/hang"); Api.GetActiveSheet().GetRange("A1").SetValue("answered"); }
    catch (e) { Api.GetActiveSheet().GetRange("A1").SetValue(e.name + ":" + Math.round((Date.now() - t0) / 1000)); }
  })();`);
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  await page.locator("#oc-net-notice-enable").click();
  expect((await run).error).toBeNull();
  expect(await input(page, 0, 0)).toMatch(/^NetworkError:1[0-1]$/);
});

test("at most 4 requests at once, and 60 a minute per workbook", async ({ page }) => {
  test.setTimeout(90_000);
  await boot(page);
  let now = 0;
  let most = 0;
  const seen = await fakeApi(page, async (route) => {
    now++;
    most = Math.max(most, now);
    await new Promise((r) => setTimeout(r, 150));
    now--;
    return route.fulfill({ status: 200, body: "ok", headers: { "access-control-allow-origin": "*" } });
  });
  const run = startMacro(page, `(async function () {
    var ps = [];
    for (var i = 0; i < 61; i++) ps.push(fetch("${API}/n" + i).then(function () { return "ok"; }, function (e) { return e.name; }));
    var out = await Promise.all(ps);
    var ok = out.filter(function (x) { return x === "ok"; }).length;
    Api.GetActiveSheet().GetRange("A1").SetValue(ok + "|" + out.filter(function (x) { return x !== "ok"; }).join(","));
  })();`);
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  await page.locator("#oc-net-notice-enable").click();
  expect((await run).error).toBeNull();
  expect(await input(page, 0, 0)).toBe("60|NetworkBlocked");
  expect(seen).toHaveLength(60);
  expect(most).toBeLessThanOrEqual(4);
  expect(most).toBe(4);
});

test("a reply is data: never a formula, never markup", async ({ page }) => {
  await boot(page);
  await fakeApi(page, (route) => route.fulfill({
    status: 200,
    headers: { "access-control-allow-origin": "*", "content-type": "text/plain" },
    body: route.request().url().endsWith("/f") ? "=1+1" : '<img src=x onerror="window.__pwned=1">',
  }));
  await defineFunctions(page, [
    "/**",
    " * @customfunction",
    " * @param {string} p",
    " * @returns {string}",
    " */",
    `async function GET(p) { return await (await fetch("${API}/" + p)).text(); }`,
    "Api.AddCustomFunction(GET);",
  ].join("\n"));
  await typeAt(page, 0, 0, '=GET("f")');
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  await page.locator("#oc-net-notice-enable").click();
  await expect.poll(() => shown(page, 0, 0), { timeout: 10_000 }).toBe("=1+1");
  await typeAt(page, 1, 0, '=GET("h")');
  await expect.poll(() => shown(page, 1, 0), { timeout: 10_000 }).toBe('<img src=x onerror="window.__pwned=1">');
  expect(await page.evaluate(() => window.__pwned)).toBeUndefined();
  expect(await page.locator("img[src='x']").count()).toBe(0);
});

// ---------------------------------------------------------------------------
// 4. The host's policy comes first.

for (const [what, caps] of [
  ["canUseNetwork: false", { canUseNetwork: false }],
  ["an allowlist without the site", { networkAllowlist: ["data.example.org"] }],
  ["an empty allowlist", { networkAllowlist: [] }],
  ["canRunMacros: false", { canRunMacros: false }],
]) {
  test(`${what}: refused, and nobody is asked`, async ({ page }) => {
    await boot(page);
    const seen = await fakeApi(page);
    await page.evaluate((c) => window.opencalcEditor.setCapabilities(c), caps);
    const res = await startMacro(page, `(async function () {
      try { await fetch("${API}/x"); Api.GetActiveSheet().GetRange("A1").SetValue("sent"); }
      catch (e) { Api.GetActiveSheet().GetRange("A1").SetValue(e.name); }
    })();`);
    expect(res.error).toBeNull();
    expect(await input(page, 0, 0)).toBe("NetworkBlocked");
    await expect(notice(page)).toBeHidden();
    await expect(bar(page)).toBeHidden();
    expect(seen).toEqual([]);
  });
}

test("an allowlist that names the site still asks the person", async ({ page }) => {
  await boot(page);
  const seen = await fakeApi(page);
  await page.evaluate(() => window.opencalcEditor.setCapabilities({ networkAllowlist: ["api.example.com"] }));
  const run = startMacro(page, MACRO);
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  await page.locator("#oc-net-notice-enable").click();
  expect((await run).error).toBeNull();
  expect(seen).toHaveLength(1);
});

test("the embedded mode refuses by default", async ({ page }) => {
  await boot(page);
  const caps = await page.evaluate(() => {
    window.opencalcEditor.setCapabilities({ mode: "embedded" });
    return window.opencalcEditor.getCapabilities();
  });
  expect(caps.canUseNetwork).toBe(false);
  expect(caps.networkAllowlist).toBeNull();
});

test("the page's Content-Security-Policy applies to the request and is not worked around", async ({ page }) => {
  await page.route("**/editor.html*", async (route) => {
    const res = await route.fetch();
    await route.fulfill({ response: res, headers: { ...res.headers(), "content-security-policy": "connect-src 'self'" } });
  });
  await boot(page);
  const seen = await fakeApi(page);
  const run = startMacro(page, `(async function () {
    try { await fetch("${API}/x"); Api.GetActiveSheet().GetRange("A1").SetValue("sent"); }
    catch (e) { Api.GetActiveSheet().GetRange("A1").SetValue(e.name); }
  })();`);
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  await page.locator("#oc-net-notice-enable").click();
  expect((await run).error).toBeNull();
  expect(await input(page, 0, 0)).toBe("NetworkError");
  expect(seen).toEqual([]);
});

// ---------------------------------------------------------------------------
// 5. Where a grant lives.

/// One part out of a ZIP package (stored or deflated).
function zipParts(buf) {
  let eocd = buf.length - 22;
  while (eocd >= 0 && buf.readUInt32LE(eocd) !== 0x06054b50) eocd--;
  const count = buf.readUInt16LE(eocd + 10);
  let at = buf.readUInt32LE(eocd + 16);
  const parts = {};
  for (let i = 0; i < count; i++) {
    const method = buf.readUInt16LE(at + 10);
    const size = buf.readUInt32LE(at + 20);
    const nameLen = buf.readUInt16LE(at + 28);
    const extra = buf.readUInt16LE(at + 30);
    const comment = buf.readUInt16LE(at + 32);
    const local = buf.readUInt32LE(at + 42);
    const name = buf.toString("utf8", at + 46, at + 46 + nameLen);
    const start = local + 30 + buf.readUInt16LE(local + 26) + buf.readUInt16LE(local + 28);
    const data = buf.subarray(start, start + size);
    parts[name] = method === 8 ? zlib.inflateRawSync(data) : Buffer.from(data);
    at += 46 + nameLen + extra + comment;
  }
  return parts;
}

const grants = (page) => page.evaluate(() => JSON.parse(localStorage.getItem("opencalc.network-grants.v1") || "[]"));

test("a grant is kept on this device only: not in the saved file, not an edit a co-editor receives", async ({ page }) => {
  await boot(page);
  await fakeApi(page);
  await defineFunctions(page, FXRATE);
  await typeAt(page, 0, 0, "=FXRATE()");
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  const undoBefore = await page.evaluate(() => window.opencalcEditor.wasmApi().session_undo_mark());
  await page.locator("#oc-net-notice-enable").click();
  await expect.poll(() => shown(page, 0, 0), { timeout: 10_000 }).toBe("1.5");

  const [g] = await grants(page);
  expect(g.site).toBe("https://api.example.com");
  expect(g.hash).toMatch(/^[0-9a-f]{64}$/);
  // Enable made no edit, so there was nothing to send a co-editor.
  expect(await page.evaluate(() => window.opencalcEditor.wasmApi().session_undo_mark())).toBe(undoBefore);
  // Nothing of it reaches the workbook a co-editor joins from …
  const snap = Buffer.from(await page.evaluate(() => Array.from(window.opencalcEditor.wasmApi().session_snapshot())));
  expect(snap.includes(Buffer.from(g.hash))).toBe(false);
  expect(snap.includes(Buffer.from("network-grants"))).toBe(false);
  // … or the file.
  for (const fmt of ["xlsx", "xlsm"]) {
    const bytes = Buffer.from(await page.evaluate((f) => Array.from(window.opencalcEditor.wasmApi().session_save_as(f)), fmt));
    for (const [name, part] of Object.entries(zipParts(bytes))) {
      for (const needle of [g.hash, g.doc, "network-grants", "grant"]) {
        expect(part.includes(Buffer.from(needle)), `${fmt} ${name} carries "${needle}"`).toBe(false);
      }
    }
  }
});

test("an opened file keeps its grant across a reload; Trust Center lists it and Remove asks again", async ({ page }) => {
  await boot(page);
  const seen = await fakeApi(page);
  await page.evaluate(() => {
    const api = window.opencalcEditor.wasmApi();
    api.session_set_cell(0, 2, 2, "kept");
  });
  // A workbook with a macro, saved and opened as a file.
  const bytes = await page.evaluate(() => Array.from(window.opencalcEditor.wasmApi().session_save_as("xlsx")));
  const open = () => page.evaluate((b) => window.opencalcEditor.openBytes(new Uint8Array(b), "rates.xlsx"), bytes);
  await open();
  let run = startMacro(page, MACRO);
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  await page.locator("#oc-net-notice-enable").click();
  expect((await run).error).toBeNull();
  expect(await grants(page)).toHaveLength(1);

  await page.reload();
  await expect(page.locator("#tb-status")).toHaveAttribute("data-engine", /^\d/, { timeout: 30_000 });
  await open();
  expect((await startMacro(page, MACRO)).error).toBeNull();
  await expect(notice(page)).toBeHidden();
  expect(seen).toHaveLength(2);

  // A different workbook with the same macro is asked.
  await page.evaluate(() => window.opencalcEditor.wasmApi().session_new());
  run = startMacro(page, MACRO);
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  await page.locator("#oc-net-notice-disable").click();
  expect((await run).error.name).toBe("NetworkBlocked");

  // File ▸ Options ▸ Trust Center.
  await open();
  await page.evaluate(() => window.opencalcEditor.runCommand("file.trust-center"));
  await expect(page.locator("#oc-modal-title")).toHaveText("Trust Center");
  await expect(page.locator("#oc-tc-external")).toContainText("https://api.example.com");
  await expect(page.locator("#oc-tc-external")).toContainText('Macro "Fetch rates" in rates.xlsx');
  // As in Excel's Trust Center, nothing changes until OK: Cancel keeps it …
  expect(await page.locator("#oc-modal .oc-confirm-actions button").allTextContents()).toEqual(["OK", "Cancel"]);
  await page.locator("#oc-tc-grants button", { hasText: "Remove" }).click();
  await expect(page.locator("#oc-tc-none")).toBeVisible();
  expect(await grants(page)).toHaveLength(1);
  await page.locator("#oc-modal .oc-confirm-actions button", { hasText: "Cancel" }).click();
  expect(await grants(page)).toHaveLength(1);
  // … and OK applies it.
  await page.evaluate(() => window.opencalcEditor.runCommand("file.trust-center"));
  await page.locator("#oc-tc-grants button", { hasText: "Remove" }).click();
  await page.locator("#oc-modal .oc-confirm-actions button", { hasText: "OK" }).click();
  expect(await grants(page)).toEqual([]);
  run = startMacro(page, MACRO);
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  await page.locator("#oc-net-notice-disable").click();
  await run;
});

test("editing a source drops its old grants", async ({ page }) => {
  await boot(page);
  await fakeApi(page);
  let run = startMacro(page, MACRO, "M");
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  await page.locator("#oc-net-notice-enable").click();
  await run;
  expect(await grants(page)).toHaveLength(1);
  run = startMacro(page, MACRO.replace("A1", "B1"), "M");
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  // The edited macro's first request already dropped the old grant.
  expect(await grants(page)).toEqual([]);
  await page.locator("#oc-net-notice-enable").click();
  await run;
  expect(await grants(page)).toHaveLength(1);
});

test("without storage, a grant lasts for the page only", async ({ page }) => {
  // Storage refused for the grants (as a private window or blocked site data
  // refuses it), the rest of the editor left alone.
  await page.addInitScript(() => {
    const deny = () => { throw new DOMException("denied", "SecurityError"); };
    const { getItem, setItem } = Storage.prototype;
    Storage.prototype.getItem = function (k) { return k === "opencalc.network-grants.v1" ? deny() : getItem.call(this, k); };
    Storage.prototype.setItem = function (k, v) { return k === "opencalc.network-grants.v1" ? deny() : setItem.call(this, k, v); };
  });
  await boot(page);
  const seen = await fakeApi(page);
  const run = startMacro(page, MACRO);
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  await page.locator("#oc-net-notice-enable").click();
  expect((await run).error).toBeNull();
  expect((await startMacro(page, MACRO)).error).toBeNull();
  await expect(notice(page)).toBeHidden();
  expect(seen).toHaveLength(2);
  expect(await page.evaluate(() => window.opencalcEditor.listNetworkGrants().length)).toBe(1);
});

test("the Security Notice is raised over the Macros window, and Escape there is Disable, not Close", async ({ page }) => {
  await boot(page);
  await fakeApi(page);
  await page.keyboard.press("Alt+F8");
  await page.locator("#oc-mw-new").click();
  await page.locator("#oc-mw-code").fill(MACRO);
  await page.locator("#oc-mw-run").click();
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  const z = await page.evaluate(() => [getComputedStyle(document.getElementById("oc-net-notice")).zIndex, getComputedStyle(document.getElementById("oc-modal")).zIndex].map(Number));
  expect(z[0]).toBeGreaterThan(z[1]);
  await page.keyboard.press("Escape");
  await expect(notice(page)).toBeHidden();
  // The Macros window is still open, showing why the run failed.
  await expect(page.locator("#oc-mw-result")).toContainText("NetworkBlocked", { timeout: 10_000 });
});

test("Enable Content on the Security Warning bar allows what was disabled", async ({ page }) => {
  await boot(page);
  const seen = await fakeApi(page);
  const run = startMacro(page, MACRO);
  await expect(notice(page)).toBeVisible({ timeout: 10_000 });
  await page.locator("#oc-net-notice-disable").click();
  expect((await run).error.name).toBe("NetworkBlocked");
  await expect(page.locator("#oc-net-bar-text")).toContainText('The macro "Fetch rates" wants to connect to https://api.example.com');
  await page.locator("#oc-net-bar-enable").click();
  await expect(bar(page)).toBeHidden();
  expect((await startMacro(page, MACRO)).error).toBeNull();
  await expect(notice(page)).toBeHidden();
  expect(seen).toHaveLength(1);
});
