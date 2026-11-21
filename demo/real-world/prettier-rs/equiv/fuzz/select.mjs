// Picks findings to minimize: up to K per (category, idem, lang) bucket, shortest input first,
// soundness findings also by Prettier's error message; skipping the expected `nonidem_same_as_prettier` class unless --all.
// Usage: node select.mjs FINDINGS.jsonl... [--k 6] > picked.jsonl
import fs from "node:fs";
const argv = process.argv.slice(2);
const ki = argv.indexOf("--k");
const K = ki >= 0 ? Number(argv.splice(ki, 2)[1]) : 6;
const all = argv.includes("--all");
const files = argv.filter((a) => !a.startsWith("--"));
const buckets = {};
for (const f of files)
  for (const l of fs.readFileSync(f, "utf8").split("\n")) {
    if (!l) continue;
    const x = JSON.parse(l);
    if (!all && x.category === "equal" && x.idem === "nonidem_same_as_prettier") continue;
    if (x.category === "prettier_timeout") continue;
    const sig = x.category === "SOUNDNESS" ? "/" + (x.prettier.err ?? "").split("\n")[0].replace(/\(\d+:\d+\)/g, "").slice(0, 80) : "";
    (buckets[`${x.category}/${x.idem}/${x.lang}${sig}`] ??= []).push(x);
  }
for (const [k, xs] of Object.entries(buckets)) {
  xs.sort((a, b) => a.stdin.length - b.stdin.length);
  // spread picks over distinct option sets
  const picked = [], seen = new Set();
  for (const x of xs) {
    const key = x.name.split("~")[0] + " " + x.args.slice(2).join(" ");
    if (seen.has(key)) continue;
    seen.add(key); picked.push(x);
    if (picked.length >= K) break;
  }
  console.error(k, xs.length, "->", picked.length);
  for (const x of picked) console.log(JSON.stringify(x));
}
