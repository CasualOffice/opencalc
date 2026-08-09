import { useCallback, useRef, useState } from "react";
import { OpenCalcSheet } from "./OpenCalcSheet";

// Config objects are hoisted out of the component. Defining them inline would
// make a new object every render — harmless here because the wrapper compares
// by value, but it is the habit that causes the remount bug in wrappers that
// do not.
const ENGINE = { calculation: "auto" };
const UI = {
  chrome: { header: false },
  theme: {
    light: { accentColor: "#7c3aed" },
    dark: { accentColor: "#a78bfa" },
  },
};

export default function App() {
  const sheet = useRef(null);
  const [cell, setCell] = useState("A1");
  const [dirty, setDirty] = useState(false);

  const onSelectionChanged = useCallback((e) => {
    setCell(`R${e.activeCell.row + 1}C${e.activeCell.col + 1}`);
  }, []);

  // `source` matters: without it, persisting on change and loading on mount
  // makes the host echo its own writes back to itself.
  const onCellsChanged = useCallback((e) => {
    if (e.source !== "api") setDirty(true);
  }, []);

  const download = async () => {
    const bytes = await sheet.current.save();
    const url = URL.createObjectURL(new Blob([bytes]));
    Object.assign(document.createElement("a"), { href: url, download: "workbook.xlsx" }).click();
    URL.revokeObjectURL(url);
    setDirty(false);
  };

  return (
    <div style={{ height: "100vh", display: "flex", flexDirection: "column" }}>
      <header style={{ display: "flex", gap: 12, alignItems: "center", padding: "10px 14px", borderBottom: "1px solid #e4e7ec" }}>
        <strong>Quarterly model</strong>
        <span style={{ color: "#667" }}>{cell}</span>
        <button onClick={download} style={{ marginLeft: "auto" }}>
          {dirty ? "Save changes" : "Download"}
        </button>
      </header>

      <OpenCalcSheet
        ref={sheet}
        style={{ flex: 1, minHeight: 0 }}
        engine={ENGINE}
        ui={UI}
        onSelectionChanged={onSelectionChanged}
        onCellsChanged={onCellsChanged}
      />
    </div>
  );
}
