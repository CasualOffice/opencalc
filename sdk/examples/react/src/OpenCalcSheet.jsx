// The React wrapper, in full. It is short on purpose — the element does the
// work and this only bridges React's model to it.
//
// Three things it has to get right, each of which is a bug in someone's React
// wrapper right now:
//
//  1. **Not remounting on every render.** Config objects are new identities each
//     render, so a naive effect tears the engine down and rebuilds it — losing
//     the workbook — every time the parent re-renders. Config is applied
//     imperatively and compared by value.
//  2. **Strict Mode's double mount.** `useEffect` runs twice in development.
//     Mounting is idempotent and the cleanup is real, or you get two engines
//     and a leak that only shows up in dev.
//  3. **Events.** React's synthetic system does not carry custom DOM events, so
//     listeners are attached directly and torn down on change.
//
// On React 19 object props reach a custom element as *properties*; on 18 and
// earlier they stringify to "[object Object]". This never passes objects as
// props, so it behaves the same on both.
import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import "@opencalc/sheet";

const sameJson = (a, b) => JSON.stringify(a ?? null) === JSON.stringify(b ?? null);

export const OpenCalcSheet = forwardRef(function OpenCalcSheet(
  { engine, ui, onReady, onCellsChanged, onSelectionChanged, style, className },
  ref,
) {
  const host = useRef(null);
  const applied = useRef({});

  // The imperative API, for hosts that need to drive it: open, save, execute.
  useImperativeHandle(ref, () => host.current, []);

  // Configuration, applied rather than remounted. Compared by value because a
  // literal like `ui={{ chrome: { header: false } }}` is a new object on every
  // render and would otherwise reapply — or worse, remount — forever.
  useEffect(() => {
    const el = host.current;
    if (!el) return;
    if (!sameJson(engine, applied.current.engine)) {
      applied.current.engine = engine;
      if (engine) el.configure(engine);
    }
    if (!sameJson(ui?.theme, applied.current.theme)) {
      applied.current.theme = ui?.theme;
      el.resetTheme();
      if (ui?.theme) el.theme(ui.theme);
    }
    if (!sameJson(ui?.chrome, applied.current.chrome)) {
      applied.current.chrome = ui?.chrome;
      if (ui?.chrome) el.chrome(ui.chrome);
    }
    if (!sameJson(ui?.commands, applied.current.commands)) {
      applied.current.commands = ui?.commands;
      if (ui?.commands) el.commands(ui.commands);
    }
    if (ui?.colorScheme !== applied.current.colorScheme) {
      applied.current.colorScheme = ui?.colorScheme;
      el.setColorScheme(ui?.colorScheme ?? "auto");
    }
  }, [engine, ui]);

  // Listeners: attached directly, and returned unsubscribers are awaited
  // because `on()` resolves after mount.
  useEffect(() => {
    const el = host.current;
    if (!el) return undefined;
    const pending = [];
    const bind = (name, handler) => {
      if (handler) pending.push(el.on(name, handler));
    };
    bind("cellsChanged", onCellsChanged);
    bind("selectionChanged", onSelectionChanged);
    if (onReady) el.ready.then(() => onReady(el));
    return () => {
      for (const p of pending) Promise.resolve(p).then((off) => off?.());
    };
  }, [onCellsChanged, onSelectionChanged, onReady]);

  return <opencalc-sheet ref={host} style={style} class={className} />;
});
