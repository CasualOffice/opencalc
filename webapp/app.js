// OpenCalc WebAssembly demo — loads the engine and wires the interactive bits.
import init, {
  version,
  eval_formula,
  render_xlsx,
  describe_xlsx,
} from "./pkg/casual_calc_wasm.js";

const DPI = 96;

async function main() {
  await init();
  document.getElementById("version").textContent = `WebAssembly demo · v${version()}`;

  wireFormula();
  wireXlsx();
}

function wireFormula() {
  const input = document.getElementById("formula");
  const result = document.getElementById("formula-result");
  const run = () => {
    const out = eval_formula(input.value);
    result.textContent = out === "" ? "(empty)" : out;
  };
  document.getElementById("eval").addEventListener("click", run);
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") run();
  });
  for (const chip of document.querySelectorAll(".chip")) {
    chip.addEventListener("click", () => {
      input.value = chip.dataset.f;
      run();
    });
  }
  run();
}

function wireXlsx() {
  const status = document.getElementById("xlsx-status");
  const img = document.getElementById("grid");

  const renderBytes = (bytes) => {
    try {
      status.textContent = describe_xlsx(bytes);
      const png = render_xlsx(bytes, 640, 360, DPI);
      const blob = new Blob([png], { type: "image/png" });
      img.src = URL.createObjectURL(blob);
    } catch (err) {
      status.textContent = `Error: ${err}`;
    }
  };

  document.getElementById("sample").addEventListener("click", async () => {
    status.textContent = "Loading sample…";
    const resp = await fetch("./sample.xlsx");
    const bytes = new Uint8Array(await resp.arrayBuffer());
    renderBytes(bytes);
  });

  document.getElementById("file").addEventListener("change", async (e) => {
    const file = e.target.files[0];
    if (!file) return;
    const bytes = new Uint8Array(await file.arrayBuffer());
    renderBytes(bytes);
  });

  // Drag & drop onto the page.
  document.body.addEventListener("dragover", (e) => e.preventDefault());
  document.body.addEventListener("drop", async (e) => {
    e.preventDefault();
    const file = e.dataTransfer.files[0];
    if (!file) return;
    const bytes = new Uint8Array(await file.arrayBuffer());
    renderBytes(bytes);
  });
}

main().catch((err) => {
  document.getElementById("xlsx-status").textContent = `Failed to load engine: ${err}`;
});
