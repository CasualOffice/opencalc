// Painting a `DisplayList` onto the canvas.
//
// This is the point of `RND-10`. The canvas painted from a per-cell payload and
// the PNG renderer painted from a `DisplayList`, so every primitive existed
// twice — once in Rust and once in JavaScript — and a divergence between them
// was invisible until somebody compared what was on screen to what came out of
// an export. Charts were the clearest case: `drawPie`, `drawBarChart`,
// `drawLineChart`, `drawAxes` and `drawLegend` here, against
// `casual_calc_layout::chart::push_chart` there, drawing the same picture.
//
// What made this defensible to do now rather than in principle: the cost was
// measured rather than assumed. The row deferred it on the claim that "a naive
// per-frame serialisation would be slower than what it replaces", and nothing
// had checked. Laying out and serialising a full viewport is ~178 µs natively,
// and a real frame across the WebAssembly boundary is ~0.6 ms for 1434 items —
// about 3.6% of the 16.67 ms a 60 fps frame allows.
//
// Deliberately a *painter* and not a renderer: it takes ready-made items and
// puts them on a context. It decides nothing about layout, which is what keeps
// the engine the only thing that knows where anything goes.

import { fontStack } from "./editor.paint.js";

/// Paint every item of a display list onto `ctx`, back to front.
///
/// The list arrives in painter's order, so this must not reorder it — a legend
/// drawn before its wedges would vanish under them.
///
/// **A display list is in twips, and a canvas is in pixels.** The two
/// conversions are not the same and cannot be done with one `ctx.scale`:
/// geometry is `twips × dpi / 1440`, and a font size is `points × dpi / 72`.
/// Scaling the context would apply the first to both and render every label at
/// a fifteenth of its size. `casual-calc-render` does exactly these two sums —
/// `twips_to_px` and `font_pt * dpi / 72` — and this matching them is what
/// makes "the canvas and the PNG draw from the same list" true rather than
/// approximately true.
export function paintList(ctx, list, dpi = 96) {
  if (!list || !Array.isArray(list.items)) return;
  const u = { geo: dpi / 1440, pt: dpi / 72 };
  for (const item of list.items) paintItem(ctx, item, u);
}

/// One item. Unknown kinds are skipped rather than throwing.
///
/// A display list is produced by a newer engine than the page may have loaded —
/// the wasm and the JavaScript are versioned together, but a cached bundle can
/// straddle a deploy. A primitive nobody here can draw yet is a gap in the
/// picture; an exception is a blank canvas.
function paintItem(ctx, item, u) {
  if (!item || typeof item !== "object") return;
  const kind = Object.keys(item)[0];
  const v = item[kind];
  switch (kind) {
    case "polyline":
      return polyline(ctx, v, u);
    case "polygon":
      return polygon(ctx, v, u);
    case "wedge":
      return wedge(ctx, v, u);
    case "text":
      return text(ctx, v, u);
    default:
      return undefined;
  }
}

function polyline(ctx, v, u) {
  if (!v.points || v.points.length < 2) return;
  ctx.save();
  ctx.strokeStyle = css(v.color);
  // A width below one device pixel is a hairline, not an invisible line.
  ctx.lineWidth = Math.max(1, v.width * u.geo);
  ctx.lineJoin = "round";
  ctx.lineCap = "round";
  ctx.beginPath();
  v.points.forEach((p, i) =>
    i ? ctx.lineTo(p.x * u.geo, p.y * u.geo) : ctx.moveTo(p.x * u.geo, p.y * u.geo),
  );
  ctx.stroke();
  ctx.restore();
}

function polygon(ctx, v, u) {
  if (!v.points || v.points.length < 3) return;
  ctx.save();
  ctx.fillStyle = css(v.fill);
  ctx.beginPath();
  v.points.forEach((p, i) =>
    i ? ctx.lineTo(p.x * u.geo, p.y * u.geo) : ctx.moveTo(p.x * u.geo, p.y * u.geo),
  );
  ctx.closePath();
  ctx.fill();
  ctx.restore();
}

function wedge(ctx, v, u) {
  const r = v.radius * u.geo;
  if (!(r > 0)) return;
  const inner = v.innerRadius > 0 ? v.innerRadius * u.geo : 0;
  const from = v.from;
  const to = v.from + v.sweep;
  ctx.save();
  ctx.fillStyle = css(v.fill);
  ctx.beginPath();
  if (inner > 0) {
    // A doughnut: out along the near edge, round the rim, back along the far
    // edge and round the hole the other way, so the hole is not filled.
    ctx.arc(v.center.x * u.geo, v.center.y * u.geo, r, from, to);
    ctx.arc(v.center.x * u.geo, v.center.y * u.geo, inner, to, from, true);
  } else {
    ctx.moveTo(v.center.x * u.geo, v.center.y * u.geo);
    ctx.arc(v.center.x * u.geo, v.center.y * u.geo, r, from, to);
  }
  ctx.closePath();
  ctx.fill();
  ctx.restore();
}

function text(ctx, v, u) {
  if (!v.content) return;
  ctx.save();
  ctx.fillStyle = css(v.color);
  const weight = v.bold ? "600 " : "";
  const style = v.italic ? "italic " : "";
  // Points, not twips: `font_pt * dpi / 72`, the same sum the renderer does.
  ctx.font = `${style}${weight}${(v.fontPt * u.pt).toFixed(2)}px ${fontStack(v.fontName)}`;
  ctx.textBaseline = "middle";
  const r = { x: v.rect.x * u.geo, y: v.rect.y * u.geo, w: v.rect.w * u.geo, h: v.rect.h * u.geo };
  // The engine gives a *box* and an alignment, not a baseline — so the same
  // item lands identically wherever it is drawn.
  const y = r.y + r.h / 2;
  if (v.align === "center") {
    ctx.textAlign = "center";
    ctx.fillText(v.content, r.x + r.w / 2, y);
  } else if (v.align === "right") {
    ctx.textAlign = "right";
    ctx.fillText(v.content, r.x + r.w, y);
  } else {
    ctx.textAlign = "left";
    ctx.fillText(v.content, r.x, y);
  }
  ctx.restore();
}

/// Colours arrive as `RRGGBB` with no `#`, the way the model stores them.
function css(hex) {
  if (!hex) return "#000";
  return hex.startsWith("#") ? hex : `#${hex}`;
}
