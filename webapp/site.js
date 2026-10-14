// The site's motion and small interactions. Vanilla, no dependencies, and
// never imported by the editor.
//
// - With prefers-reduced-motion, or without IntersectionObserver, nothing here
//   animates: `html.motion` is never set, every reveal sits in its final state,
//   and the hero story keeps the finished markup it ships with.
// - Otherwise: reveals on scroll, counters count up, illustrations loop only
//   while on screen, and the hero story plays while visible, with a Pause
//   button, stopping in a hidden tab.
// - In both modes: the mobile menu closes on Escape and on navigation, and the
//   docs table of contents marks the section in view.
(function () {
  "use strict";
  var root = document.documentElement;
  var reduce = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  var IO = "IntersectionObserver" in window;
  var motion = !reduce && IO;

  function all(sel, ctx) { return Array.prototype.slice.call((ctx || document).querySelectorAll(sel)); }
  function inView(el) { var r = el.getBoundingClientRect(); return r.top < innerHeight && r.bottom > 0; }

  // ---------------------------------------------------------------- counters
  function count(el) {
    var to = parseInt(el.getAttribute("data-count"), 10), t0 = null, dur = 1200;
    if (!to) return;
    var fmt = function (n) { return n.toLocaleString("en-US"); };
    el.textContent = "0";
    function tick(t) {
      if (t0 === null) t0 = t;
      var p = Math.min((t - t0) / dur, 1), e = 1 - Math.pow(1 - p, 3);
      el.textContent = fmt(Math.round(to * e));
      if (p < 1) requestAnimationFrame(tick);
    }
    requestAnimationFrame(tick);
  }

  // ---------------------------------------------------------------- reveals
  function reveals() {
    var once = all(".rv, .seq, .pipe, .phases, .il-wrap, .bigmark, [data-count]");
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) {
        if (!e.isIntersecting) return;
        e.target.classList.add("in");
        if (e.target.hasAttribute("data-count")) count(e.target);
        io.unobserve(e.target);
      });
    }, { rootMargin: "0px 0px -8% 0px", threshold: 0.12 });
    once.forEach(function (el) {
      if (inView(el) && !el.hasAttribute("data-count")) el.classList.add("in");
      else io.observe(el);
    });

    // Loops run only while their drawing is on screen.
    var live = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) { e.target.classList.toggle("live", e.isIntersecting && !document.hidden); });
    }, { threshold: 0.2 });
    all(".il-wrap").forEach(function (el) { live.observe(el); });
    document.addEventListener("visibilitychange", function () {
      if (document.hidden) all(".il-wrap.live").forEach(function (el) { el.classList.remove("live"); });
      else all(".il-wrap.in").forEach(function (el) { if (inView(el)) el.classList.add("live"); });
    });

    // Last, so the elements already on screen were marked first.
    root.classList.add("motion");
  }

  // ---------------------------------------------------------------- hero story
  function story(el) {
    var DUR = [2600, 3000, 3000, 3600, 3200];
    var LAST = DUR.length - 1;
    var btn = el.querySelector(".story-toggle");
    var label = btn && btn.querySelector("span");
    var step = -1, timer = null, paused = false, visible = false;

    function setStep(n) {
      step = n;
      el.setAttribute("data-step", String(n));
      for (var k = 0; k <= LAST; k++) el.classList.toggle("s" + k, k <= n);
    }
    function reset() {
      el.classList.add("reset");
      setStep(-1);
      void el.offsetWidth;
      el.classList.remove("reset");
    }
    function next() {
      var n = step + 1;
      if (n > LAST) { reset(); n = 0; }
      setStep(n);
      timer = setTimeout(next, DUR[n]);
    }
    function sync() {
      var run = visible && !paused && !document.hidden;
      if (run && !timer) timer = setTimeout(next, step < 0 ? 350 : 1200);
      if (!run && timer) { clearTimeout(timer); timer = null; }
    }

    reset();
    if (btn) {
      btn.hidden = false;
      btn.addEventListener("click", function () {
        paused = !paused;
        btn.setAttribute("aria-pressed", paused ? "true" : "false");
        if (label) label.textContent = paused ? "Play" : "Pause";
        btn.classList.toggle("is-paused", paused);
        sync();
      });
    }
    new IntersectionObserver(function (es) { visible = es[0].isIntersecting; sync(); }, { threshold: 0.25 }).observe(el);
    document.addEventListener("visibilitychange", sync);
  }

  // ---------------------------------------------------------------- mobile menu
  function menu(box) {
    function close() { box.open = false; }
    box.addEventListener("keydown", function (e) {
      if (e.key === "Escape" && box.open) { close(); box.querySelector("summary").focus(); }
    });
    all("a", box).forEach(function (a) { a.addEventListener("click", close); });
    document.addEventListener("click", function (e) { if (box.open && !box.contains(e.target)) close(); });
  }

  // ---------------------------------------------------------------- docs TOC
  function toc(nav) {
    var links = all('a[href^="#"]', nav), queued = false;
    var targets = links.map(function (a) { return document.getElementById(a.getAttribute("href").slice(1)); });
    function update() {
      queued = false;
      var cur = -1;
      targets.forEach(function (t, i) { if (t && t.getBoundingClientRect().top < innerHeight * 0.3) cur = i; });
      links.forEach(function (a, i) {
        if (i === cur) a.setAttribute("aria-current", "true"); else a.removeAttribute("aria-current");
      });
    }
    addEventListener("scroll", function () { if (!queued) { queued = true; requestAnimationFrame(update); } }, { passive: true });
    update();
  }

  function init() {
    if (motion) {
      reveals();
      all("[data-story]").forEach(story);
    }
    all("details.menu").forEach(menu);
    all("nav.toc").forEach(toc);
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", init); else init();
})();
