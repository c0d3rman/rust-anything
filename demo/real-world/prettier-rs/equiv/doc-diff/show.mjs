// Prints both canonical docs of one input, and both final strings, for inspecting a mismatch.
//   node show.mjs <file> [--width N] [--strict] -- <prettier CLI flags incl. --stdin-filepath>
//   node show.mjs --case <name> [--inputs inputs.jsonl] [--width N] [--strict] [--rebind]
// --inputs defaults to $DOC_DIFF_INPUTS.
// Writes /dev/stdout: first difference, then `prettier.json` / `port.json` next to --out (default
// ./out/show-*).
import fs from "node:fs";
import path from "node:path";
import { requireEnv, prettier, flagsToOptions, prettierDoc, fromWire, wireString, Canon, firstDiff, short, Port } from "./lib.mjs";

const argv = process.argv.slice(2);
const opt = (k) => {
  const i = argv.indexOf(k);
  return i === -1 ? undefined : argv[i + 1];
};
let args, stdin;
if (opt("--case")) {
  const inputs = opt("--inputs") ?? requireEnv("DOC_DIFF_INPUTS", "the inputs JSONL file that holds --case (or pass --inputs)");
  const found = fs
    .readFileSync(inputs, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((l) => JSON.parse(l))
    .find((c) => c.name === opt("--case"));
  if (!found) {
    console.error(`show: no case named ${opt("--case")} in ${inputs}`);
    process.exit(2);
  }
  ({ args, stdin } = found);
} else {
  stdin = fs.readFileSync(argv[0], "utf8");
  args = argv.slice(argv.indexOf("--") + 1);
}
const width = opt("--width") === undefined ? undefined : Number(opt("--width"));
const strict = argv.includes("--strict");
const rebind = argv.includes("--rebind");
const opts = flagsToOptions(args);
if (width !== undefined) opts.printWidth = width;
const port = new Port();
const r = await port.ask({ args, stdin, doc: true, fmt: true, ...(width !== undefined ? { width } : {}) });
port.close();
const canon = new Canon();
const p = canon.root(await prettierDoc(stdin, opts), strict, rebind);
const q = r.status === "ok" ? canon.root(fromWire(r.doc), strict, rebind) : null;
const a = canon.expand(p, { n: 1e6 });
const b = q === null ? r : canon.expand(q, { n: 1e6 });
const outDir = opt("--out") ?? "out";
fs.mkdirSync(outDir, { recursive: true });
fs.writeFileSync(path.join(outDir, "show-prettier.json"), JSON.stringify(a, null, 1));
fs.writeFileSync(path.join(outDir, "show-port.json"), JSON.stringify(b, null, 1));
const d = p === q ? null : firstDiff(a, b);
console.log(p === q ? "docs equal" : `docs differ at ${d?.path}\n  prettier: ${short(d?.a, 2000)}\n  port:     ${short(d?.b, 2000)}`);
let pf;
try {
  pf = await prettier.format(stdin, opts);
} catch (e) {
  pf = "ERR " + e.message;
}
const rf = r.fmt?.ok !== undefined ? wireString(r.fmt.ok) : "ERR " + r.fmt?.err;
console.log(pf === rf ? "strings equal" : `strings differ\n--- prettier\n${pf}\n--- port\n${rf}`);
