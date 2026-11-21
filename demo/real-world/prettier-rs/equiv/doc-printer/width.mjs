// getStringWidth equivalence: every Unicode scalar value as a one-character string, then a
// structured set of multi-code-point strings. Usage: node width.mjs [--random N] [--seed S]
import fs from "node:fs";
import { rust, getStringWidth, here } from "./lib.mjs";

const args = process.argv.slice(2);
const opt = (k, d) => { const i = args.indexOf(k); return i < 0 ? d : Number(args[i + 1]); };
const mismatches = [];
const report = (s, js, rs, set) => {
  if (mismatches.length < 200) mismatches.push({ set, cps: [...s].map(c => c.codePointAt(0).toString(16)), js, rust: rs });
};

// 1. every scalar value
const t0 = Date.now();
const all = rust.widthAllScalars();
let scalars = 0, bad1 = 0;
for (let cp = 0; cp <= 0x10ffff; cp++) {
  if (cp >= 0xd800 && cp <= 0xdfff) continue;
  scalars++;
  const s = String.fromCodePoint(cp);
  const js = getStringWidth(s);
  if (js !== all[cp]) { bad1++; report(s, js, all[cp], "scalar"); }
}
console.log(`scalars: ${scalars} checked, ${bad1} mismatches (${Date.now() - t0} ms)`);

// 2. structured strings
const cp = (...c) => String.fromCodePoint(...c);
const range = (a, b) => Array.from({ length: b - a + 1 }, (_, i) => a + i);
const ZWJ = cp(0x200d), VS16 = cp(0xfe0f), VS15 = cp(0xfe0e), KEYCAP = cp(0x20e3);
const tones = range(0x1f3fb, 0x1f3ff).map(c => cp(c));
const RI = range(0x1f1e6, 0x1f1ff).map(c => cp(c));
const tags = range(0xe0020, 0xe007f).map(c => cp(c));
const combining = [0x300, 0x301, 0x308, 0x34f, 0x36f, 0x370, 0x483, 0x591, 0x64b, 0x93c, 0x1ab0, 0x1dc0, 0x20d0, 0x20e3, 0xfe20, 0x302a, 0x3099].map(c => cp(c));
// Code points that emoji-regex treats specially, or whose width is interesting.
const emojiBlocks = [[0x2190, 0x21ff], [0x2300, 0x23ff], [0x2460, 0x24ff], [0x25a0, 0x27bf], [0x2900, 0x297f], [0x2b00, 0x2bff], [0x3030, 0x303d], [0x3297, 0x3299], [0x1f000, 0x1faff]];
const emojiCps = emojiBlocks.flatMap(([a, b]) => range(a, b)).map(c => cp(c)).concat(["#", "*", ...range(0x30, 0x39).map(c => cp(c)), cp(0xa9), cp(0xae), cp(0x203c), cp(0x2049), cp(0x2122), cp(0x2139)]);
const people = [0x1f468, 0x1f469, 0x1f466, 0x1f467, 0x1f9d1, 0x1f474, 0x1f475, 0x1f476, 0x1f46e, 0x1f477, 0x1f3c3, 0x1f3ca, 0x26f9, 0x1f3cc, 0x1f44d, 0x270c, 0x261d].map(c => cp(c));
const zwjTails = [0x2764, 0x1f48b, 0x1f91d, 0x2640, 0x2642, 0x2695, 0x2696, 0x2708, 0x1f33e, 0x1f373, 0x1f393, 0x1f3a4, 0x1f3eb, 0x1f4bb, 0x1f527, 0x1f52c, 0x1f680, 0x1f692, 0x1f9af, 0x1f9bc, 0x1f9bd, 0x1f466, 0x1f467, 0x1f468, 0x1f469, 0x1f9d1, 0x1f525, 0x1fa79, 0x2b1b, 0x1f308, 0x26a7, 0x1f5e8, 0x1f441].map(c => cp(c));
const misc = ["a", "Z", " ", "~", "\x7f", "\x1f", "\t", "\n", "\0", cp(0x80), cp(0x9f), cp(0xa0), cp(0xad), "é", "字", "한", "ｆ", "ｱ", cp(0x1100), cp(0x1160), cp(0x11a8), cp(0x200b), cp(0x200c), ZWJ, cp(0x2028), cp(0x2060), cp(0xfeff), VS15, VS16, cp(0xe0100), cp(0x3000), cp(0x303f), cp(0xff61), cp(0x20000), cp(0x2fffd), cp(0x30000), cp(0xf0000), cp(0xf07ff), cp(0x100000), cp(0x10fffd), cp(0x10ffff), cp(0x1f1e6), cp(0x1f3fb), cp(0x1f600), cp(0x2764), cp(0x263a), cp(0x1f170), cp(0x1f004), cp(0x231a), cp(0x2b50), cp(0xe0067), cp(0xe007f)];

const strings = new Set();
const add = s => strings.add(s);
// all pairs and triples over a pool of interesting code points
const pool = [...new Set([...misc, ...tones, ...combining, RI[0], RI[19], RI[17], tags[0x47], tags[0x5f], KEYCAP, people[0], people[13], zwjTails[0], zwjTails[3]])];
for (const a of pool) for (const b of pool) { add(a + b); for (const c of pool) add(a + b + c); }
// every emoji-ish code point with each modifier / selector / joiner
const suffixes = ["", VS16, VS15, KEYCAP, VS16 + KEYCAP, ...tones, ZWJ, ZWJ + cp(0x2764) + VS16, ...combining.slice(0, 4), "a", "字", ZWJ + cp(0x1f469)];
for (const e of emojiCps) for (const s of suffixes) { add(e + s); add("x" + e + s + "y"); }
// regional indicators: all pairs, and all pairs followed by a third
for (const a of RI) for (const b of RI) { add(a + b); add(a + b + "a"); for (const c of RI) add(a + b + c); }
// ZWJ sequences: person (+tone) ZWJ tail (+VS16), and families
for (const p of people) for (const t of ["", ...tones]) for (const z of zwjTails) for (const v of ["", VS16]) {
  add(p + t + ZWJ + z + v); add(p + t + ZWJ + z + v + ZWJ + p);
}
for (const a of people.slice(0, 5)) for (const b of people.slice(0, 5)) for (const c of people.slice(0, 5)) for (const d of ["", ZWJ + people[2]]) add(a + ZWJ + b + ZWJ + c + d);
// tag sequences (subdivision flags) and broken ones
for (const sub of ["gbeng", "gbsct", "gbwls", "usca", "x", ""]) {
  const tagged = [...sub].map(ch => cp(0xe0000 + ch.charCodeAt(0))).join("");
  for (const base of [cp(0x1f3f4), "a", cp(0x1f600)]) for (const end of [cp(0xe007f), ""]) add(base + tagged + end);
}
// keycaps
for (const k of ["#", "*", ..."0123456789", "a"]) for (const s of ["", VS16, VS15]) { add(k + s + KEYCAP); add(k + s); }
// combining marks stacked on various bases
for (const base of ["a", "字", cp(0x1f600), " ", "\t", ""]) for (const m1 of combining) for (const m2 of ["", ...combining]) add(base + m1 + m2);
// CJK / Hangul / fullwidth mixed with ASCII and control chars
const mixPool = ["a", "b", " ", "字", "日", "本", "한", "ｆ", "ｱ", "\t", "\x01", "\x7f", cp(0x85), cp(0x300), "😀", cp(0x20000), "é"];
for (const a of mixPool) for (const b of mixPool) for (const c of mixPool) for (const d of mixPool) add(a + b + c + d);
// random strings over the union of all pools
let seed = opt("--seed", 1) >>> 0;
const rnd = n => { seed = (seed * 1664525 + 1013904223) >>> 0; return seed % n; };
const big = [...new Set([...pool, ...mixPool, ...emojiCps.filter((_, i) => i % 7 === 0), ...RI, ...tones, ...tags.filter((_, i) => i % 9 === 0), ...people, ...zwjTails])];
const nRandom = opt("--random", 300000);
for (let i = 0; i < nRandom; i++) { let s = ""; const len = 1 + rnd(12); for (let j = 0; j < len; j++) s += big[rnd(big.length)]; add(s); }

const list = [...strings];
let bad2 = 0;
const t1 = Date.now();
for (let i = 0; i < list.length; i += 50000) {
  const chunk = list.slice(i, i + 50000);
  const rs = rust.widths(chunk);
  chunk.forEach((s, j) => { const js = getStringWidth(s); if (js !== rs[j]) { bad2++; report(s, js, rs[j], "structured"); } });
}
console.log(`structured strings: ${list.length} checked (${nRandom} random draws), ${bad2} mismatches (${Date.now() - t1} ms)`);
fs.writeFileSync(`${here}/out/width-mismatches.json`, JSON.stringify(mismatches, null, 1));
if (bad1 + bad2) process.exitCode = 1;
