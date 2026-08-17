// Every method the declarations promise exists in the implementation.
//
// `tsc` proves the public surface *composes* — that a consumer can be written
// against it under `strict`. It cannot prove the surface is real: the
// declarations are hand-written beside hand-written JavaScript, so a `.d.ts`
// naming a method nobody implemented type-checks perfectly and fails at the
// first call.
//
// This is the cheap half of that bridge: textual, and deliberately so. Loading
// `embed.js` to inspect it needs a DOM, a WebAssembly build and a browser, and
// the browser suite already exercises the element's behaviour. What is missing
// there is nothing more than "does this name exist", which reading both files
// answers.

import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const declarations = await readFile(join(here, "..", "..", "webapp", "embed.d.ts"), "utf8");
const implementation = await readFile(join(here, "..", "..", "webapp", "embed.js"), "utf8");

// The class body, so interface members and top-level types are not mistaken for
// methods on the element.
const body = declarations.slice(declarations.indexOf("export declare class OpenCalcSheet"));
const classBody = body.slice(0, body.indexOf("\n}"));

const declared = [...classBody.matchAll(/^  (?:readonly )?([a-zA-Z]+)\s*[(:<]/gm)]
  .map((m) => m[1])
  .filter((name) => name !== "readonly");

if (declared.length === 0) {
  console.error("found no declared members — this check is not checking anything");
  process.exit(1);
}

const missing = declared.filter((name) => {
  // A method, a getter, or an assigned field on the class.
  const method = new RegExp(`^\\s*(?:async\\s+)?${name}\\s*\\(`, "m");
  const getter = new RegExp(`^\\s*get\\s+${name}\\s*\\(`, "m");
  return !method.test(implementation) && !getter.test(implementation);
});

if (missing.length > 0) {
  console.error(
    `embed.d.ts declares ${missing.length} member(s) that embed.js does not define: ${missing.join(", ")}`,
  );
  process.exit(1);
}

console.log(`every declared member exists: ${declared.join(", ")}`);
