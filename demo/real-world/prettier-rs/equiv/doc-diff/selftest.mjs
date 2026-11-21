// Controls for the canonicaliser, on Prettier's own docs for a sample of corpus cases.
//   node selftest.mjs <inputs.jsonl> [every-nth]
// Positive: an unshared deep copy, and copies rewritten only in ways the README lists as
// output-neutral (labels, break-parents, extra array nesting, split strings, renamed unreferenced
// group ids), must stay equal. Negative: each single printer-visible mutation must compare unequal.
import fs from "node:fs";
import { flagsToOptions, prettierDoc, Canon } from "./lib.mjs";

const [inputs, every = "37"] = process.argv.slice(2);
const cases = fs.readFileSync(inputs, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l))
  .filter((_, i) => i % Number(every) === 0);

const clone = (d, f = (x) => x) => {
  const memo = new Map();
  const rec = (d) => {
    if (d === null || typeof d !== "object") return f(d);
    if (memo.has(d)) return memo.get(d);
    let o;
    if (Array.isArray(d)) o = d.map(rec);
    else {
      o = {};
      for (const [k, v] of Object.entries(d))
        o[k] = ["contents", "breakContents", "flatContents", "parts", "expandedStates"].includes(k) ? rec(v) : v;
      if (d.expandedStates && d.expandedStates[0] === d.contents) o.contents = o.expandedStates[0];
    }
    memo.set(d, o);
    return f(o);
  };
  return rec(d);
};
// Every node, unshared positions (for mutations).
const nodes = (d) => {
  const out = [];
  const seen = new Set();
  const rec = (d) => {
    if (d === null || typeof d !== "object" || seen.has(d)) return;
    seen.add(d);
    out.push(d);
    if (Array.isArray(d)) d.forEach(rec);
    else for (const k of ["contents", "breakContents", "flatContents"]) rec(d[k]);
    if (d.parts) d.parts.forEach(rec);
    if (d.expandedStates) d.expandedStates.forEach(rec);
  };
  rec(d);
  return out;
};

let pos = 0, posFail = 0, neg = 0, negFail = 0;
const fail = [];
for (const c of cases) {
  let doc;
  try { doc = await prettierDoc(c.stdin, flagsToOptions(c.args)); } catch { continue; }
  const canon = new Canon();
  const base = canon.root(doc, false);
  const check = (d, want, what) => {
    const eq = canon.root(d, false) === base;
    if (want) { pos++; if (!eq) { posFail++; fail.push(`${c.name}: ${what} should be equal`); } }
    else { neg++; if (eq) { negFail++; fail.push(`${c.name}: ${what} should differ`); } }
  };
  check(clone(doc), true, "copy");
  // Neutral rewrites.
  check(clone(doc, (x) => (x && typeof x === "object" && !Array.isArray(x) && x.type === "indent" ? { type: "label", label: "x", contents: [x, { type: "break-parent" }, ""] } : x)), true, "labels+bp+empty");
  check(clone(doc, (x) => (typeof x === "string" && x.length > 1 && /^[ -~]*$/.test(x) ? [x.slice(0, 1), [x.slice(1)]] : x)), true, "split strings");
  if (x_refsFree(doc))
    check(clone(doc, (x) => (x && x.type === "group" && x.id ? { ...x, id: Symbol("renamed") } : x)), true, "renamed unreferenced group ids");
  // Mutations, each on a fresh copy, at up to 3 positions per kind.
  const kinds = {
    string: (n) => {
      const i = Array.isArray(n) ? n.findIndex((s) => typeof s === "string" && s) : -1;
      return i >= 0 && ((n[i] += "x"), true);
    },
    groupBreak: (n) => n.type === "group" && !n.expandedStates && ((n.break = !n.break), true),
    lineKind: (n) => n.type === "line" && !n.hard && ((n.soft = !n.soft), true),
    ifBreakSwap: (n) => n.type === "if-break" && JSON.stringify(n.breakContents) !== JSON.stringify(n.flatContents) &&
      (([n.breakContents, n.flatContents] = [n.flatContents, n.breakContents]), true),
    alignN: (n) => n.type === "align" && typeof n.n === "number" && n.n > 0 && ((n.n += 1), true),
    indentToAlign: (n) => n.type === "indent" && ((n.type = "align"), (n.n = 3), true),
    fillDrop: (n) => n.type === "fill" && n.parts.length > 2 && (n.parts.splice(0, 2), true),
  };
  for (const [kind, mut] of Object.entries(kinds)) {
    let done = 0;
    for (let i = 0; done < 3; i++) {
      const copy = clone(doc);
      const ns = nodes(copy);
      if (i >= ns.length) break;
      if (!mut(ns[i])) continue;
      check(copy, false, `${kind}@${i}`);
      done++;
    }
  }
}
function x_refsFree(doc) {
  // Renaming every group id consistently with no references renamed would break references; only
  // expect equality when no if-break / indent-if-break references a group.
  return !nodes(doc).some((n) => (n.type === "if-break" || n.type === "indent-if-break") && n.groupId);
}
console.log({ cases: cases.length, pos, posFail, neg, negFail });
for (const f of fail.slice(0, 30)) console.log(f);
