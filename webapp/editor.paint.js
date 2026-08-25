// Drawing: the grid, charts, images, collaborator marks and the text
// measurement they need.
//
// Split out of the single `editor.js` under `MNT-005`. The shared state
// stays in `editor.js` and is imported here as a live binding: every
// function below only *reads* it, which is what made the move safe to do
// without touching a single body.

import {
  CHART_HANDLE,
  FREEZE_HANDLE,
  HH,
  HW,
  REF_COLORS,
  _fontStackCache,
  anchoredRect,
  autofitRow,
  canvas,
  cellPx,
  chartDrag,
  chartDragRect,
  chartFrames,
  chartHandlePoints,
  chartSel,
  colWAt,
  colXAt,
  collabRoster,
  colors,
  ctx,
  draw,
  editSurface,
  filterButtons,
  filterInfo,
  geoItems,
  growthDirty,
  growthPrefix,
  growthRows,
  heightMemo,
  imageCache,
  invalidateGrowth,
  mirrorFor,
  ocThemeHost,
  off,
  offCanvas,
  on,
  participantColor,
  qsa,
  readColors,
  rebuildGrowth,
  refMirrors,
  refSpans,
  rowHAt,
  rowYAt,
  spanX,
  spanY,
  start,
  state,
  status,
  syncMirrorBox,
  t,
  tableTextAt,
  traceBlocks,
  traceMode,
  valueExtent,
  wasm,
  wrap,
} from "./editor.core.js";

export function imageFor(part) {
  const hit = imageCache.get(part);
  if (hit !== undefined) return hit;
  imageCache.set(part, null); // in flight; do not request it again every frame
  let url = "";
  try { url = wasm.session_image_data(part); } catch {}
  if (!url) return null;
  const img = new Image();
  img.onload = () => { imageCache.set(part, img); draw(); };
  // A part this browser cannot decode stays a blank frame rather than
  // retrying forever.
  img.onerror = () => imageCache.set(part, false);
  img.src = url;
  return null;
}

export function drawImages() {
  if (!wasm) return;
  let images = [];
  try { images = JSON.parse(wasm.session_images(state.sheet)); } catch { return; }
  for (const im of images) {
    const { x: x0, y: y0, w, h } = anchoredRect(im);
    if (offCanvas({ x: x0, y: y0, w, h })) continue;
    const img = imageFor(im.part);
    if (!img) continue;
    ctx.save();
    ctx.beginPath();
    ctx.rect(x0, y0, w, h);
    ctx.clip();
    // Fitted inside the anchor and centred, keeping the aspect ratio: an
    // anchor is a frame, and stretching a photograph to fill it is a visible
    // lie about the file.
    const scale = Math.min(w / img.naturalWidth, h / img.naturalHeight);
    const dw = img.naturalWidth * scale, dh = img.naturalHeight * scale;
    ctx.drawImage(img, x0 + (w - dw) / 2, y0 + (h - dh) / 2, dw, dh);
    ctx.restore();
  }
}

export function drawChartSelection() {
  if (chartDrag) {
    const r = chartDragRect();
    if (r) {
      ctx.save();
      ctx.strokeStyle = colors.accent || "#4472C4";
      ctx.setLineDash([4, 3]);
      ctx.lineWidth = 1.5;
      ctx.strokeRect(r.x + 0.5, r.y + 0.5, r.w, r.h);
      ctx.restore();
    }
  }
  if (!chartSel || chartSel.sheet !== state.sheet) return;
  const frame = chartFrames.find((f) => f.index === chartSel.index);
  if (!frame) return;
  ctx.save();
  ctx.strokeStyle = colors.accent || "#4472C4";
  ctx.lineWidth = 1.5;
  ctx.strokeRect(frame.x + 0.5, frame.y + 0.5, frame.w - 1, frame.h - 1);
  ctx.fillStyle = colors.bg;
  for (const [hx, hy] of chartHandlePoints(frame)) {
    ctx.beginPath();
    ctx.arc(hx, hy, CHART_HANDLE, 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();
  }
  ctx.restore();
}










export function cellFg(it) {
  if (it.fc) return "#" + it.fc;
  // A cell's own fill came from the *file*, so it is the same colour in either
  // theme — while `colors.fg` follows the theme. Reading the ink off the theme
  // therefore put near-white text on an authored pale fill the moment dark mode
  // was switched on, and the cell's contents disappeared. The fill is what the
  // text actually sits on, so that is what it has to contrast against.
  //
  // An explicit font colour above still wins: that pairing is the author's, and
  // second-guessing it would repaint somebody's deliberate formatting.
  if (it.bg) return contrastInk(it.bg);
  return tableTextAt(it.r, it.c) || colors.fg;
}

export function neededRowHeight(it, colWidth) {
  const key = `${it.t}\u0000${it.w ? 1 : 0}\u0000${it.rot || 0}\u0000${it.fs || 0}\u0000${it.b ? 1 : 0}\u0000${it.i ? 1 : 0}\u0000${it.fn || ""}\u0000${Math.round(colWidth)}`;
  const hit = heightMemo.get(key);
  if (hit !== undefined) return hit;
  const value = measureRowHeight(it, colWidth);
  // Bounded so a sheet of entirely distinct strings cannot grow it without end.
  if (heightMemo.size < 50000) heightMemo.set(key, value);
  return value;
}

export function measureRowHeight(it, colWidth) {
  // Rotated text needs vertical room, or it is clipped to a 20 px row and the
  // rotation achieves nothing. Height is the text's own length projected onto
  // the vertical axis; stacked text is one glyph per line.
  if (it.rot) {
    ctx.font = cellFont(it);
    let needed;
    if (it.rot === 255) {
      needed = [...String(it.t)].length * cellLineH(it) + 6;
    } else {
      const deg = it.rot <= 90 ? it.rot : it.rot - 90;
      needed = Math.abs(Math.sin((deg * Math.PI) / 180)) * ctx.measureText(String(it.t)).width
        + cellPx(it) + 6;
    }
    return Math.min(needed, 409); // Excel's row-height ceiling
  }
  if (it.w) {
    // Explicit newlines split first, then each segment wraps: a hard break is a
    // break whether or not the text after it would have fitted.
    const lines = String(it.t)
      .split("\n")
      .flatMap((seg) => wrapLines({ ...it, t: seg }, colWidth - 8));
    return lines.length * cellLineH(it) + 6;
  }
  // Newlines make a cell tall even without wrap, which this did not account
  // for: `autofitRow` had its own copy of this arithmetic that did, so the two
  // disagreed about the same cell — and the copy was the one missing rotation.
  // Both cases live here now.
  const hard = String(it.t).split("\n").length;
  if (hard > 1) return hard * cellLineH(it) + 6;
  // A tall font grows its row by the font's own box plus Excel's leading; at the
  // 11 pt default this comes to exactly the default row height, so an ordinary
  // styled row is left alone instead of being inflated by 25%.
  if (it.fs) return cellPx(it) + 5;
  return null;
}

export function growthBefore(row) {
  if (growthDirty) rebuildGrowth();
  if (!growthRows.length) return 0;
  // How many grown rows sit strictly above `row`.
  let lo = 0, hi = growthRows.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (growthRows[mid] < row) lo = mid + 1; else hi = mid;
  }
  return growthPrefix[lo];
}

export function fontStack(fn) {
  const key = fn || "";
  let s = _fontStackCache.get(key);
  if (s === undefined) {
    try { s = wasm.font_css_stack(key); } catch { s = "system-ui, sans-serif"; }
    _fontStackCache.set(key, s);
  }
  return s;
}

export function cellFont(it) {
  const weight = it.b ? "600 " : "";
  const slant = it.i ? "italic " : "";
  return `${slant}${weight}${cellPx(it)}px ${fontStack(it.fn)}`;
}

export function runFont(it, run) {
  const merged = {
    b: run.b ?? it.b,
    i: run.i ?? it.i,
    fn: run.fn ?? it.fn,
    // Superscript and subscript are drawn smaller, as every renderer does;
    // the format states the position, not the size.
    fs: run.fs ?? it.fs,
  };
  const px = run.va ? Math.max(7, cellPx(merged) * 0.72) : cellPx(merged);
  const weight = merged.b ? "600 " : "";
  const slant = merged.i ? "italic " : "";
  return `${slant}${weight}${px}px ${fontStack(merged.fn)}`;
}

export function runsWidth(it) {
  const saved = ctx.font;
  let total = 0;
  for (const run of it.runs) {
    ctx.font = runFont(it, run);
    total += ctx.measureText(run.t).width;
  }
  ctx.font = saved;
  return total;
}

export function drawRuns(it, x, y) {
  const saved = ctx.font;
  const savedFill = ctx.fillStyle;
  let cursor = x;
  for (const run of it.runs) {
    ctx.font = runFont(it, run);
    ctx.fillStyle = run.fc ? "#" + run.fc : (cellFg(it));
    // Superscript rides above the baseline, subscript below it. The offsets
    // are fractions of the cell's size so they track a resized font.
    const shift = run.va === "superscript" ? -cellPx(it) * 0.32
      : run.va === "subscript" ? cellPx(it) * 0.18
      : 0;
    ctx.fillText(run.t, cursor, y + shift);
    const w = ctx.measureText(run.t).width;
    if (run.u || run.st) {
      const ly = run.st ? y + shift - cellPx(it) * 0.28 : y + shift + 2.5;
      ctx.fillRect(cursor, ly, w, 1);
    }
    cursor += w;
  }
  ctx.font = saved;
  ctx.fillStyle = savedFill;
  return cursor - x;
}

export function cellLineH(it) { return cellPx(it) + 4; }

export function textY(it, yTop, h, lineH) {
  if (it.va === "t") return yTop + lineH / 2 + 2;
  if (it.va === "b") return yTop + h - lineH / 2 - 2;
  return yTop + h / 2;
}

export function wrapLines(it, maxW) {
  // An explicit line break (Alt+Enter) is a hard break: wrap each of its
  // segments separately rather than letting \s+ swallow it as a space.
  const text = String(it.t);
  if (text.includes("\n")) {
    return text.split("\n").flatMap((seg) => wrapLines({ ...it, t: seg }, maxW));
  }
  ctx.font = cellFont(it);
  const lines = [];
  let line = "";
  for (const word of text.split(/\s+/)) {
    const test = line ? line + " " + word : word;
    if (ctx.measureText(test).width <= maxW || !line) {
      if (!line && ctx.measureText(word).width > maxW) {
        let chunk = "";
        for (const ch of word) {
          if (chunk && ctx.measureText(chunk + ch).width > maxW) { lines.push(chunk); chunk = ch; }
          else chunk += ch;
        }
        line = chunk;
      } else line = test;
    } else { lines.push(line); line = word; }
  }
  if (line) lines.push(line);
  return lines.length ? lines : [""];
}

export function borderWidth(style) {
  if (style === "thick" || style === "double") return 2;
  if (style === "medium" || style === "mediumDashed") return 1.5;
  return 1; // thin, hair, dashed, dotted, …
}

export function drawEdge(spec, x0, y0, x1, y1) {
  if (!spec) return;
  const sep = spec.indexOf(":");
  const style = sep >= 0 ? spec.slice(0, sep) : spec;
  const color = sep >= 0 ? spec.slice(sep + 1) : "";
  const width = borderWidth(style);
  ctx.strokeStyle = color ? "#" + color : colors.fg;
  ctx.setLineDash(style === "dashed" || style === "mediumDashed" ? [4, 2] : style === "dotted" ? [1, 2] : []);
  // A double border is two thin parallel lines, not one thick one — drawing it
  // heavy makes it indistinguishable from `thick`, which is the very thing it
  // is chosen over for a totals rule. Offset perpendicular to the edge.
  if (style === "double") {
    ctx.lineWidth = 1;
    const vertical = Math.abs(x1 - x0) < Math.abs(y1 - y0);
    for (const d of [-1, 1]) {
      const dx = vertical ? d : 0;
      const dy = vertical ? 0 : d;
      ctx.beginPath();
      ctx.moveTo(Math.floor(x0) + dx + 0.5, Math.floor(y0) + dy + 0.5);
      ctx.lineTo(Math.floor(x1) + dx + 0.5, Math.floor(y1) + dy + 0.5);
      ctx.stroke();
    }
    ctx.setLineDash([]);
    return;
  }
  ctx.lineWidth = width;
  // A 1px line lands crisply on a half-pixel; wider lines centre on the edge.
  const off = width === 1 ? 0.5 : 0;
  ctx.beginPath();
  ctx.moveTo(Math.floor(x0) + off, Math.floor(y0) + off);
  ctx.lineTo(Math.floor(x1) + off, Math.floor(y1) + off);
  ctx.stroke();
  ctx.setLineDash([]);
}

export function quadClip(row, col, v) {
  const f = state.freeze;
  const x0 = col < f.fc ? HW : f.bodyX0, x1 = col < f.fc ? f.bodyX0 : v.w;
  const y0 = row < f.fr ? HH : f.bodyY0, y1 = row < f.fr ? f.bodyY0 : v.h;
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

export function drawFreezeHandles() {
  const F = state.freeze;
  ctx.save();
  ctx.fillStyle = colors.freezeLine;
  ctx.globalAlpha = 0.55;
  if (F.fc === 0) {
    ctx.fillRect(HW - FREEZE_HANDLE + 2, HH * 0.18, FREEZE_HANDLE - 3, HH * 0.44);
  }
  if (F.fr === 0) {
    ctx.fillRect(HW * 0.18, HH - FREEZE_HANDLE + 2, HW * 0.44, FREEZE_HANDLE - 3);
  }
  ctx.restore();
}

export function drawFreezeDividers(v) {
  const F = state.freeze;
  const drag = state.freezeDrag;
  const showCol = F.fc > 0 || (drag && drag.axis === "col");
  const showRow = F.fr > 0 || (drag && drag.axis === "row");
  if (!showCol && !showRow) return;
  ctx.save();
  const line = colors.freezeLine || "#5f6368";
  if (showCol) {
    const x = drag && drag.axis === "col" ? drag.px : F.bodyX0;
    const g = ctx.createLinearGradient(x, 0, x + 7, 0);
    g.addColorStop(0, "rgba(60,64,72,0.20)"); g.addColorStop(1, "rgba(60,64,72,0)");
    ctx.fillStyle = g; ctx.fillRect(x, 0, 7, v.h);
    ctx.strokeStyle = line; ctx.lineWidth = 2;
    ctx.beginPath(); ctx.moveTo(x - 1, 0); ctx.lineTo(x - 1, v.h); ctx.stroke();
  }
  if (showRow) {
    const y = drag && drag.axis === "row" ? drag.py : F.bodyY0;
    const g = ctx.createLinearGradient(0, y, 0, y + 7);
    g.addColorStop(0, "rgba(60,64,72,0.20)"); g.addColorStop(1, "rgba(60,64,72,0)");
    ctx.fillStyle = g; ctx.fillRect(0, y, v.w, 7);
    ctx.strokeStyle = line; ctx.lineWidth = 2;
    ctx.beginPath(); ctx.moveTo(0, y - 1); ctx.lineTo(v.w, y - 1); ctx.stroke();
  }
  ctx.restore();
}

export function drawStretched(line, x0, width, y) {
  const words = String(line).split(/\s+/).filter(Boolean);
  const prev = ctx.textAlign;
  ctx.textAlign = "left";
  if (words.length < 2) {
    ctx.fillText(String(line), x0, y);
    ctx.textAlign = prev;
    return;
  }
  const ink = words.reduce((sum, wd) => sum + ctx.measureText(wd).width, 0);
  const gap = (width - ink) / (words.length - 1);
  if (gap <= 0) {
    ctx.fillText(String(line), x0, y);
    ctx.textAlign = prev;
    return;
  }
  let wx = x0;
  for (const wd of words) {
    ctx.fillText(wd, wx, y);
    wx += ctx.measureText(wd).width + gap;
  }
  ctx.textAlign = prev;
}

export function tintColor(hex, tint) {
  const n = parseInt(hex, 16);
  const ch = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((c) =>
    tint >= 0 ? c + (255 - c) * tint : c * (1 + tint),
  );
  return ch.map((c) => Math.round(c).toString(16).padStart(2, "0")).join("").toUpperCase();
}

export function drawFilterRegion(withQuad, filterInfo) {
  const row = filterInfo.r0;
  const y = rowYAt(row);
  if (y === undefined) return;
  const ch = rowHAt(row);
  // Header fills, so the glyph can be drawn in whatever contrasts with the cell
  // it sits on — a header is usually filled, and the fill can be any colour.
  const fillOf = new Map();
  for (const it of geoItems) if (it.r === row && it.bg) fillOf.set(it.c, it.bg);

  for (let col = filterInfo.c0; col <= filterInfo.c1; col++) {
    const x = colXAt(col);
    if (x === undefined) continue;
    const cw = colWAt(col);
    // Skip a column too narrow to hold the glyph without covering its label.
    if (cw < 22) continue;
    const size = 12;
    const bx = Math.round(x + cw - size - 4), by = Math.round(y + (ch - size) / 2);
    const active = filterInfo.cols.has(col);
    const ink = contrastInk(fillOf.get(col));
    withQuad(row, col, () => {
      ctx.save();
      // Following Sheets: an inverted triangle while the column is merely
      // filterable, a funnel once a rule is on it — the shape carries the state,
      // so it survives being small and needs no colour cue.
      //
      // Both are filled. A 1px outline at this size renders muddy once the
      // canvas transform (dpr x zoom) puts its edges off-pixel; a solid shape
      // stays crisp at any scale.
      const mx = bx + size / 2, my = by + size / 2;
      ctx.fillStyle = ink;
      ctx.globalAlpha = active ? 1 : 0.75;
      ctx.beginPath();
      if (active) {
        // Symmetric funnel: flat top, sides converging to a straight stem.
        ctx.moveTo(mx - 5, my - 4.5);
        ctx.lineTo(mx + 5, my - 4.5);
        ctx.lineTo(mx + 1.6, my - 0.6);
        ctx.lineTo(mx + 1.6, my + 4.5);
        ctx.lineTo(mx - 1.6, my + 4.5);
        ctx.lineTo(mx - 1.6, my - 0.6);
      } else {
        ctx.moveTo(mx - 4, my - 2.2);
        ctx.lineTo(mx + 4, my - 2.2);
        ctx.lineTo(mx, my + 2.8);
      }
      ctx.closePath();
      ctx.fill();
      ctx.restore();
    });
    filterButtons.push({ x: bx, y: by, w: size, h: size, col, row });
  }
}

export function contrastInk(hex) {
  if (!hex || hex.length < 6) return colors.fg;
  const lin = (c) => {
    const v = parseInt(hex.slice(c, c + 2), 16) / 255;
    return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
  };
  const l = 0.2126 * lin(0) + 0.7152 * lin(2) + 0.0722 * lin(4);
  return l > 0.179 ? "#111418" : "#ffffff";
}

export function drawTraceArrows(withQuad) {
  if (!traceBlocks.length) return;
  const tx = colXAt(state.sel.col), ty = rowYAt(state.sel.row);
  if (tx === undefined || ty === undefined) return;
  const tcx = tx + colWAt(state.sel.col) / 2, tcy = ty + rowHAt(state.sel.row) / 2;
  ctx.save();
  ctx.strokeStyle = traceMode === "dep" ? "#a142f4" : "#1a73e8";
  ctx.fillStyle = ctx.strokeStyle;
  ctx.lineWidth = 1.5;
  for (const b of traceBlocks) {
    const x = colXAt(b.c0), y = rowYAt(b.r0);
    if (x === undefined || y === undefined) continue;
    const x1 = (colXAt(b.c1) ?? x) + colWAt(b.c1);
    const y1 = (rowYAt(b.r1) ?? y) + rowHAt(b.r1);
    withQuad(b.r0, b.c0, () => {
      ctx.strokeRect(x + 0.5, y + 0.5, x1 - x - 1, y1 - y - 1);
      // Arrow from the block's centre to the traced cell, or the other way for
      // dependents — the direction *is* the information.
      const bcx = (x + x1) / 2, bcy = (y + y1) / 2;
      const [fx, fy, txx, tyy] = traceMode === "dep"
        ? [tcx, tcy, bcx, bcy]
        : [bcx, bcy, tcx, tcy];
      ctx.beginPath();
      ctx.moveTo(fx, fy);
      ctx.lineTo(txx, tyy);
      ctx.stroke();
      const ang = Math.atan2(tyy - fy, txx - fx);
      ctx.beginPath();
      ctx.moveTo(txx, tyy);
      ctx.lineTo(txx - 8 * Math.cos(ang - 0.4), tyy - 8 * Math.sin(ang - 0.4));
      ctx.lineTo(txx - 8 * Math.cos(ang + 0.4), tyy - 8 * Math.sin(ang + 0.4));
      ctx.closePath();
      ctx.fill();
    });
  }
  ctx.restore();
}

export function paintRefTokens() {
  const surface = editSurface;
  if (!surface) return;
  const m = refMirrors.get(surface);
  // Nothing to tint: drop the mirror's content and give the surface its own
  // text back, so a plain value never depends on the mirror being right.
  if (!refSpans.length) {
    if (m) { m.textContent = ""; m.style.display = "none"; }
    surface.classList.remove("tinted");
    return;
  }
  const mirror = mirrorFor(surface);
  syncMirrorBox(surface, mirror);
  mirror.style.display = "";
  surface.classList.add("tinted");

  const text = surface.value;
  // Spans are character offsets from the engine's scanner, in order.
  const parts = [];
  let at = 0;
  refSpans.forEach((r, i) => {
    const start = Math.max(at, Math.min(r.s, text.length));
    const end = Math.max(start, Math.min(r.e, text.length));
    if (start > at) parts.push([text.slice(at, start), null]);
    parts.push([text.slice(start, end), REF_COLORS[i % REF_COLORS.length]]);
    at = end;
  });
  parts.push([text.slice(at), null]);
  mirror.textContent = "";
  for (const [chunk, color] of parts) {
    if (!chunk) continue;
    const node = document.createElement("span");
    node.textContent = chunk;
    if (color) node.style.color = color;
    mirror.appendChild(node);
  }
  // A long formula scrolls inside the surface; the mirror has to follow or the
  // tint slides off the tokens it belongs to.
  mirror.scrollLeft = surface.scrollLeft;
  mirror.scrollTop = surface.scrollTop;
}

export function applyTheme(theme) {
  if (theme === "auto") delete ocThemeHost.dataset.theme;
  else ocThemeHost.dataset.theme = theme;
  localStorage.setItem("oc-theme", theme);
  readColors();
  draw();
}

export function applyAccent(color) {
  ocThemeHost.style.setProperty("--oc-accent-color", color);
  localStorage.setItem("oc-accent", color);
  for (const b of qsa("#set-accent button")) {
    b.setAttribute("aria-current", b.dataset.c === color ? "true" : "false");
  }
  readColors();
  draw();
}

export function refreshTheme() {
  readColors();
  invalidateGrowth();
  draw();
}

export function drawCollaborators(v, perQuad) {
  if (!collabRoster.size) return;
  for (const who of collabRoster.values()) {
    // Only this sheet. A cursor on another tab is real and is not here.
    if (who.sheet !== state.sheet) continue;
    const color = participantColor(who.color);
    // Before the selection, and not inside its visibility test: while a formula
    // is being written the selection walks off to pick references, so the cell
    // being typed into can be on screen when the cursor is not. The draft is the
    // part somebody needs to see.
    drawCollaboratorDraft(who, color, v, perQuad);

    const sel = who.selection;
    if (!Array.isArray(sel) || sel.length !== 4) continue;
    const [r0, c0, r1, c1] = sel;
    const sx = spanX(Math.min(c0, c1), Math.max(c0, c1), v);
    const sy = spanY(Math.min(r0, r1), Math.max(r0, r1), v);
    // Scrolled out of view, or collapsed to nothing by the pane clamp.
    if (sx.w <= 0 || sy.h <= 0) continue;

    perQuad(() => {
      ctx.save();
      ctx.strokeStyle = color;
      ctx.lineWidth = 2;
      ctx.strokeRect(sx.x + 1, sy.y + 1, Math.max(1, sx.w - 1), Math.max(1, sy.h - 1));
      // A wash, so a range reads as theirs without hiding the values in it.
      ctx.globalAlpha = 0.12;
      ctx.fillStyle = color;
      ctx.fillRect(sx.x + 1, sy.y + 1, Math.max(1, sx.w - 1), Math.max(1, sy.h - 1));
      ctx.restore();
    });

    const name = typeof who.name === "string" && who.name ? who.name : "someone";
    perQuad(() => {
      ctx.save();
      // Both of these leak from cell drawing, which sets them per cell. The
      // label came out centred on its own left edge — "Guest 21" rendered as
      // "st 21", the rest of it painted white-on-white outside the tag — and
      // nothing about the code said so. Only the screenshot did.
      ctx.textAlign = "left";
      ctx.textBaseline = "middle";
      ctx.font = "11px system-ui, sans-serif";
      const tw = Math.ceil(ctx.measureText(name).width);
      const tagW = tw + 8;
      const tagH = 15;
      // Above the range, and below it when that would leave the grid — a label
      // clipped off the top of the canvas names nobody.
      const above = sy.y - tagH >= HH;
      const ty = above ? sy.y - tagH : sy.y + sy.h;
      // Clamped so a range running off the right edge keeps its label on screen.
      const tx = Math.max(HW, Math.min(sx.x, v.w - tagW));
      ctx.fillStyle = color;
      ctx.fillRect(tx, ty, tagW, tagH);
      ctx.fillStyle = "#fff";
      ctx.fillText(name, tx + 4, ty + tagH / 2 + 0.5);
      ctx.restore();
    });
  }
}

export function drawCollaboratorDraft(who, color, v, perQuad) {
  const draft = who.editing;
  if (!draft || !Array.isArray(draft.at) || draft.at.length !== 2) return;
  const [row, col] = draft.at;
  const dx = spanX(col, col, v);
  const dy = spanY(row, row, v);
  // Scrolled out of view, or hidden, or clamped away by a frozen pane.
  if (dx.w <= 0 || dy.h <= 0) return;
  const text = typeof draft.text === "string" ? draft.text : "";

  perQuad(() => {
    ctx.save();
    // Opaque, so the committed value underneath does not show through and
    // double up with the draft over it.
    ctx.fillStyle = colors.bg;
    ctx.fillRect(dx.x, dy.y, dx.w, dy.h);
    ctx.strokeStyle = color;
    ctx.lineWidth = 2;
    ctx.strokeRect(dx.x + 1, dy.y + 1, Math.max(1, dx.w - 2), Math.max(1, dy.h - 2));

    // Clipped to the cell. A long formula must not spill across the neighbours
    // the way a committed overflowing value may — those cells belong to other
    // people's values, and this text is not committed to anything.
    ctx.beginPath();
    ctx.rect(dx.x + 2, dy.y + 2, Math.max(0, dx.w - 4), Math.max(0, dy.h - 4));
    ctx.clip();
    ctx.fillStyle = colors.fg;
    ctx.textAlign = "left";
    ctx.textBaseline = "middle";
    ctx.font = "12px system-ui, sans-serif";
    // The first line only. A draft with a hard line break in it is legitimate
    // (Alt+Enter) and a preview an inch tall is not.
    const line = text.split("\n")[0];
    ctx.fillText(line, dx.x + 4, dy.y + dy.h / 2);
    // A caret at the end of it, so an empty draft still reads as "somebody is
    // in here typing" rather than as a blank cell with a border.
    const caret = Math.min(dx.x + 4 + Math.ceil(ctx.measureText(line).width) + 1, dx.x + dx.w - 3);
    ctx.fillStyle = color;
    ctx.fillRect(caret, dy.y + 3, 1.5, Math.max(2, dy.h - 6));
    ctx.restore();
  });
}

export async function registerSuppliedFonts() {
  const asked = new URL(location.href).searchParams.get("fonts");
  if (asked === null) return;
  const service = asked === "" ? "/api/fonts" : asked;
  try {
    const response = await fetch(service, { headers: { accept: "application/json" } });
    if (!response.ok) {
      console.warn(`[opencalc] font service ${service} answered ${response.status}`);
      return;
    }
    // URLs, taken as given and resolved against this page. The service decides
    // where its faces live; this only fetches them.
    const { fonts = [] } = await response.json();
    for (const url of fonts) {
      try {
        const face = await fetch(new URL(url, location.href));
        if (!face.ok) {
          console.warn(`[opencalc] ${url} answered ${face.status}; not registered`);
          continue;
        }
        const bytes = new Uint8Array(await face.arrayBuffer());
        if (!wasm.register_font(bytes)) {
          console.warn(`[opencalc] ${url} is not a readable font face; ignored`);
        }
      } catch (why) {
        console.warn(`[opencalc] could not register ${url}:`, why);
      }
    }
  } catch (why) {
    console.warn(`[opencalc] font service ${service} unreachable:`, why);
  }
}
