// Prints a table per run directory: per generator and language, the count in each result
// category, idempotence counts, and the 95% upper bound on the mismatch rate (3/N, N = inputs
// both sides formatted) where no mismatch or soundness bug was seen.
// Usage: node report.mjs RUN_DIR...
import fs from "node:fs";
import path from "node:path";

const CATS = ["equal", "MISMATCH", "SOUNDNESS", "refused", "refused_opts", "both_error", "RUST_PANIC",
  "RUST_CRASH", "RUST_TIMEOUT", "prettier_timeout"];
const IDEM = ["idem_ok", "nonidem_same_as_prettier", "nonidem_diverges", "nonidem_in_other",
  "idem_second_refused", "idem_second_panic"];
const rows = [];
for (const dir of process.argv.slice(2)) {
  const s = JSON.parse(fs.readFileSync(path.join(dir, "summary.json"), "utf8"));
  for (const [g, langs] of Object.entries(s.categories)) {
    for (const [l, c] of Object.entries(langs)) {
      const idem = s.idempotence[g]?.[l] ?? {};
      const total = Object.values(c).reduce((a, b) => a + b, 0);
      const compared = (c.equal ?? 0) + (c.MISMATCH ?? 0);
      const bad = (c.MISMATCH ?? 0) + (c.SOUNDNESS ?? 0) + (c.RUST_PANIC ?? 0) + (c.RUST_CRASH ?? 0) + (c.RUST_TIMEOUT ?? 0);
      rows.push({ gen: g, lang: l, total, ...Object.fromEntries(CATS.map((k) => [k, c[k] ?? 0])),
        ...Object.fromEntries(IDEM.map((k) => [k, idem[k] ?? 0])),
        bound95: bad === 0 && compared > 0 ? (3 / compared).toExponential(2) : "-" });
    }
  }
}
const cols = Object.keys(rows[0] ?? {}).filter((k) => rows.some((r) => r[k] !== 0));
console.log("| " + cols.join(" | ") + " |");
console.log("|" + cols.map(() => "---").join("|") + "|");
for (const r of rows) console.log("| " + cols.map((k) => r[k]).join(" | ") + " |");
