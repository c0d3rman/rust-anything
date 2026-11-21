// Appends a minimized finding to ../cases.jsonl as a `{name, args, stdin}` line (the corpus
// format), and prints both sides' outputs so the finding can be described.
// Usage: node save-case.mjs MINIMIZED.jsonl INDEX NAME   (CASES overrides the target file)
import fs from "node:fs";
import path from "node:path";

const [file, idx, name] = process.argv.slice(2);
let x = fs.readFileSync(file, "utf8").trim().split("\n").map((l) => JSON.parse(l))[Number(idx)];
// A second-pass divergence is a mismatch on the first output: save that instead.
if (x.idem === "nonidem_diverges")
  x = { ...x, category: x.prettier2?.ok ? "MISMATCH" : "SOUNDNESS", stdin: x.rust.out, rust: x.rust.second, prettier: x.prettier2 };
const target = process.env.CASES ?? path.join(path.dirname(new URL(import.meta.url).pathname), "..", "cases.jsonl");
const existing = fs.existsSync(target) ? fs.readFileSync(target, "utf8").split("\n").filter(Boolean) : [];
if (existing.some((l) => JSON.parse(l).name === name)) {
  console.error(`${name} is already in ${target}`);
  process.exit(1);
}
fs.appendFileSync(target, JSON.stringify({ name, args: x.args, stdin: x.stdin }) + "\n");
const rust = x.rust.status === "ok" ? x.rust.out : `(${x.rust.status}${x.rust.reason ? ": " + x.rust.reason : ""})`;
const prettier = x.prettier?.ok ? x.prettier.out : `(error: ${x.prettier?.err ?? "timeout or crash"})`;
console.log(`${name}: ${x.category} ${x.idem ?? ""}\n--- prettier\n${prettier}\n--- rust\n${rust}`);
