// Strings with lone surrogates cannot reach `string_width` directly (Rust strings are UTF-8), so
// they are checked through the printer: a doc whose layout depends on the string's width, wired
// with the port's `{"$utf16": [...]}` marker. Usage: node lone-surrogates.mjs
import { rust, printDocToString } from "./lib.mjs";

const units = [0x61, 0xd800, 0xdbff, 0xdc00, 0xdfff, 0xd83d, 0xde00, 0x5b57, 0x301, 0x200d];
const strs = [];
for (const a of units) for (const b of units) for (const c of units) strs.push([a, b, c]);
const isLone = u => { const s = String.fromCharCode(...u); return !s.isWellFormed(); };
let checked = 0, bad = 0, badCollide = 0;
// a well-formed pair encoding U+10F800..U+10FFFF (high unit 0xDBFE/0xDBFF)
const collides = s => [...s].some(ch => ch.codePointAt(0) >= 0x10f800);
for (const u of strs.filter(isLone)) {
  const s = String.fromCharCode(...u);
  for (const w of [0, 1, 2, 3, 4, 5, 6, 7]) {
    // group([s, line, "x"]) breaks iff width(s) + 2 > w; fill exercises fits too
    const js = printDocToString({ type: "group", contents: [s, { type: "line" }, "x", { type: "fill", parts: [s, { type: "line" }, s] }] }, { printWidth: w, tabWidth: 2, useTabs: false });
    const wire = { $utf16: u };
    const r = JSON.parse(rust.printMany(JSON.stringify({ type: "group", contents: [wire, { type: "line" }, "x", { type: "fill", parts: [wire, { type: "line" }, wire] }] }), JSON.stringify([{ printWidth: w, tabWidth: 2, useTabs: false }])))[0];
    const rf = typeof r[1] === "string" ? r[1] : String.fromCharCode(...r[1].$utf16);
    checked++;
    if (r[0] !== "ok" || rf !== js.formatted) { bad++; if (collides(s)) badCollide++; if (bad < 10) console.log("mismatch", u.map(x => x.toString(16)), w, JSON.stringify(js.formatted), JSON.stringify(r)); }
  }
}
console.log(`lone-surrogate strings: ${strs.filter(isLone).length} strings x 8 widths = ${checked} prints, ${bad} mismatches (${badCollide} of them in strings that also hold a code point in U+10F800..U+10FFFF)`);
