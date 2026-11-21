// Prints `prettier-js-rs/src/id_chars.rs`: the non-ASCII code points Prettier's parsers take as
// identifier characters, from the `@babel/parser` and `typescript` in a Prettier clone.
//   node scripts/gen-id-chars.mjs <prettier clone> > prettier-js-rs/src/id_chars.rs
import fs from "node:fs"; import path from "node:path"; import { createRequire } from "node:module";
const root = process.argv[2];
// @babel/parser bundles `isIdentifierStart`/`isIdentifierChar` without exporting them: evaluate
// the bundle's copy (its tables through the two functions).
const src = fs.readFileSync(path.join(root, "node_modules/@babel/parser/lib/index.js"), "utf8");
const body = src.slice(src.indexOf("let nonASCIIidentifierStartChars"), src.indexOf("const reservedWords = {"));
const babel = new Function(`${body}; return { isIdentifierStart, isIdentifierChar };`)();
const ts = createRequire(path.join(root, "package.json"))("typescript");
const ranges = f => {
  const out = []; let start = -1;
  for (let c = 0x80; c <= 0x110000; c++) {
    const ok = c < 0x110000 && !(c >= 0xd800 && c <= 0xdfff) && f(c);
    if (ok && start < 0) start = c;
    if (!ok && start >= 0) { out.push([start, c - 1]); start = -1; }
  }
  return out;
};
const table = (name, f) => `pub(crate) const ${name}: &[(u32, u32)] = &[\n${
  ranges(f).map(([a, b]) => `    (0x${a.toString(16)}, 0x${b.toString(16)}),\n`).join("")}];\n`;
const esNext = ts.ScriptTarget.ESNext;
process.stdout.write(
  "//! Non-ASCII identifier characters as Prettier's parsers accept them, generated from\n" +
  "//! `@babel/parser` (`isIdentifierStart`/`isIdentifierChar`) and `typescript`\n" +
  "//! (`isIdentifierStart`/`isIdentifierPart`, ES next). Inclusive ranges, sorted.\n\n" +
  table("BABEL_ID_START", babel.isIdentifierStart) +
  table("BABEL_ID_CONTINUE", babel.isIdentifierChar) +
  table("TS_ID_START", c => ts.isIdentifierStart(c, esNext)) +
  table("TS_ID_CONTINUE", c => ts.isIdentifierPart(c, esNext)));
