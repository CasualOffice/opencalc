"use client";

import { useEffect, useState } from "react";

export function SheetClient() {
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    // Imported inside the effect, not at module scope: importing registers a
    // custom element, which must happen in the browser. Strict Mode runs this
    // twice in development; registration is guarded, and the flag below keeps
    // the element out of the tree until the definition exists so it does not
    // render as an unknown tag first.
    let cancelled = false;
    import("@opencalc/sheet").then(() => {
      if (!cancelled) setLoaded(true);
    });
    return () => { cancelled = true; };
  }, []);

  if (!loaded) return <div style={{ height: 600, background: "#f7f8fa", borderRadius: 12 }} />;

  // `assets-url` set declaratively, not in an effect: the element reads it when
  // it mounts, and an effect that sets it afterwards is already too late.
  // Turbopack does not resolve `.wasm` from `import.meta.url`, so the files are
  // copied into `public/` by the `postinstall` in package.json.
  return (
    <opencalc-sheet
      assets-url="/opencalc/"
      style={{ display: "block", height: 600 }}
    />
  );
}
