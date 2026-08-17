// The React surface, compiled the way a host writes it.

import { useRef } from "react";
import { OpenCalcSheet } from "@opencalc/react";
import type { OpenCalcSheetProps } from "@opencalc/react";
import type { OpenCalcSheet as OpenCalcSheetElement } from "@opencalc/sheet";

export function Editor(props: OpenCalcSheetProps) {
  // The ref is the custom element itself, so a host that needs to drive it
  // imperatively gets the same API the plain element offers.
  const ref = useRef<OpenCalcSheetElement>(null);

  const save = async () => {
    const bytes = await ref.current?.save();
    return bytes;
  };
  void save;

  return (
    <OpenCalcSheet
      ref={ref}
      engine={{ access: "view", calculation: "automatic" }}
      ui={{
        theme: { accentColor: "#1f6f4a" },
        chrome: { toolbar: false },
        commands: { disabled: ["insert.chart"] },
      }}
      onReady={() => undefined}
      onCellsChanged={(event) => {
        if (event.source === "api") return;
        void event.range.c1;
      }}
      style={{ height: "600px" }}
      className="sheet"
      {...props}
    />
  );
}
