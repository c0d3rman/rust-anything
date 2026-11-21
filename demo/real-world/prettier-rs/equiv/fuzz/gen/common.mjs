// Shared helpers for the generators: seeded RNG, seed loading, output.
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";

export function requireEnv(name, what) {
  const v = process.env[name];
  if (!v) {
    console.error(`Set ${name} to ${what}.`);
    process.exit(2);
  }
  return v;
}
export const EQUIV = requireEnv("PRETTIER_EQUIV", "the directory holding prettier/ (Prettier 3.6.2, built) and corpus/inputs.jsonl");
export const SEEDS = path.join(EQUIV, "corpus/inputs.jsonl");
// Parsers come from the Prettier checkout's node_modules (the versions Prettier 3.6.2 uses).
export const requireP = createRequire(path.join(EQUIV, "prettier/package.json"));

export function rng(seed) {
  let a = seed >>> 0;
  const next = () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  const r = {
    next,
    int: (n) => Math.floor(next() * n),
    pick: (xs) => xs[Math.floor(next() * xs.length)],
    chance: (p) => next() < p,
  };
  return r;
}

export function filepathOf(args) {
  const i = args.indexOf("--stdin-filepath");
  return i >= 0 ? args[i + 1] : "";
}

export function loadSeeds(file = SEEDS) {
  return fs.readFileSync(file, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l));
}

export function argValue(argv, name, dflt) {
  const i = argv.indexOf(name);
  return i >= 0 ? argv[i + 1] : dflt;
}

// Writes JSON lines to stdout, waiting for the pipe to drain.
export async function emit(obj) {
  if (!process.stdout.write(JSON.stringify(obj) + "\n")) {
    await new Promise((r) => process.stdout.once("drain", r));
  }
}
process.stdout.on("error", (e) => { if (e.code === "EPIPE") process.exit(0); });

// Lines of stdin split on "\n" only (readline also splits on U+2028/U+2029, which JSON leaves raw).
export async function* stdinLines() {
  let buf = "";
  process.stdin.setEncoding("utf8");
  for await (const chunk of process.stdin) {
    buf += chunk;
    let i;
    while ((i = buf.indexOf("\n")) >= 0) { yield buf.slice(0, i); buf = buf.slice(i + 1); }
  }
  if (buf) yield buf;
}
