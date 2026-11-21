// node regexpp-corpus.cjs <eslint clone> <app-cases dir> <out.jsonl>: every regex literal and RegExp string in the recorded
// inputs plus generated patterns, with regexpp's validatePattern result and parse tree for each; read by
// `REGEXPP_CORPUS=<out.jsonl> cargo test --lib regexpp::tests::corpus`.
const fs = require("fs");
const path = require("path");
const E = process.argv[2];
const casesDir = process.argv[3];
const out = process.argv[4];
const { RegExpValidator, RegExpParser } = require(path.join(E, "node_modules/@eslint-community/regexpp"));
const espree = require(path.join(E, "node_modules/espree"));

const patterns = new Map();
const flagsSet = new Set(["", "u", "v", "uv", "gimsuyd", "gg", "x", "uu", "dgimsvy"]);
function addPat(p, src) { if (typeof p === "string" && !patterns.has(p)) patterns.set(p, src); }

let files = fs.readdirSync(casesDir).filter(f => f.endsWith(".stdin"));
for (const f of files) {
  const text = fs.readFileSync(path.join(casesDir, f), "utf8");
  let ast;
  for (const sourceType of ["module", "script"]) {
    try { ast = espree.parse(text, { ecmaVersion: "latest", sourceType, ecmaFeatures: { jsx: true }, tokens: true }); break; } catch {}
  }
  if (!ast) continue;
  for (const t of ast.tokens) if (t.type === "RegularExpression") { addPat(t.regex.pattern, "lit"); flagsSet.add(t.regex.flags); }
  (function walk(n) {
    if (!n || typeof n.type !== "string") return;
    if ((n.type === "CallExpression" || n.type === "NewExpression") && n.callee.type === "Identifier" && n.callee.name === "RegExp") {
      const [a, b] = n.arguments;
      if (a && a.type === "Literal" && typeof a.value === "string") addPat(a.value, "str");
      if (a && a.type === "TemplateLiteral" && a.expressions.length === 0) addPat(a.quasis[0].value.cooked, "str");
      if (b && b.type === "Literal" && typeof b.value === "string") flagsSet.add(b.value);
    }
    for (const k of Object.keys(n)) {
      if (k === "parent") continue;
      const v = n[k];
      if (Array.isArray(v)) v.forEach(walk); else if (v && typeof v === "object") walk(v);
    }
  })(ast);
}
const fromCases = patterns.size;

let seed = 12345;
function rnd(n) { seed = (seed + 0x6D2B79F5) | 0; let t = Math.imul(seed ^ (seed >>> 15), 1 | seed); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return (((t ^ (t >>> 14)) >>> 0) % n); }
const atoms = ["a", "b", "\\", "[", "]", "(", ")", "{", "}", "|", "?", "*", "+", ".", "^", "$", "-", "&&", "--", "\\q{", "\\p{L}", "\\P{Lu}", "\\p{Script=Greek}", "\\p{RGI_Emoji}", "\\k<a>", "(?<a>", "(?<b>", "(?:", "(?=", "(?!", "(?<=", "(?<!", "(?i:", "(?-i:", "(?ims-:", "(?i-i:", "\\1", "\\2", "\\10", "\\0", "\\00", "\\x4", "\\x41", "\\u00", "\\u0041", "\\u{1F600}", "\\u{110000}", "\\ud83d\\ude00", "\\ud83d", "\\c", "\\cA", "\\c1", "\\c_", "\\d", "\\D", "\\w", "\\s", "\\b", "\\B", "\\-", "\\/", "/", "\\.", "\\a", "\\z", "\\k", "{1}", "{1,}", "{2,1}", "{1,2}", "{,", "0", "1", "9", "😀", "\ud83d", "́", "👍", "🇯🇵", "‍", "!!", "##", " ", "  ", "\n", "\x1f", "\\x1f", "=", "<", ">", "_", "$", "&", "~", "%", "\\^", "\\&", "\\!", "\\#", ":", ",", "\\q{abc|d}", "[^", "[[", "]]", "\\p", "\\p{", "\\p{Lu", "\\p{gc=Lu}", "\\p{Foo}", "\\p{ASCII}", "\\p{Basic_Emoji}", "(?<𝒜>", "(?<\\u{1d49c}>", "(?<a\\u0062>", "(?<$>", "\\k<𝒜>"];
const gen = new Set();
const base = [...patterns.keys()];
for (let i = 0; i < 8000; i++) {
  let p;
  if (base.length && rnd(3) > 0) {
    p = base[rnd(base.length)];
    const ops = 1 + rnd(3);
    for (let j = 0; j < ops; j++) {
      const pos = rnd(p.length + 1);
      const op = rnd(3);
      if (op === 0) p = p.slice(0, pos) + atoms[rnd(atoms.length)] + p.slice(pos);
      else if (op === 1) p = p.slice(0, pos) + p.slice(pos + 1 + rnd(3));
      else p = p.slice(0, pos) + atoms[rnd(atoms.length)] + p.slice(pos + 1);
    }
  } else {
    p = "";
    const len = rnd(8);
    for (let j = 0; j < len; j++) p += atoms[rnd(atoms.length)];
  }
  if (p.length < 300) gen.add(p);
}
for (const p of gen) addPat(p, "gen");
for (let i = 0; i < 300; i++) {
  let f = ""; const len = rnd(5);
  for (let j = 0; j < len; j++) f += "dgimsuvyxzUé"[rnd(13)];
  flagsSet.add(f);
}

function ser(n) {
  let s = `${n.type}@${n.start}-${n.end}`;
  switch (n.type) {
    case "Character": s += `:${n.value}`; break;
    case "CharacterSet": s += `:${n.kind}:${!!n.negate}:${n.key ?? ""}:${n.value ?? ""}:${!!n.strings}`; break;
    case "Quantifier": s += `:${n.min}:${n.max}:${n.greedy}`; break;
    case "Assertion": s += `:${n.kind}:${!!n.negate}`; break;
    case "CapturingGroup": s += `:${n.name ?? ""}:${n.references.map(r => r.start).join(",")}`; break;
    case "Backreference": s += `:${n.ref}:${n.ambiguous}:${[n.resolved].flat().map(g => g.start).join(",")}`; break;
    case "CharacterClass": s += `:${n.negate}:${n.unicodeSets}`; break;
    case "ExpressionCharacterClass": s += `:${n.negate}`; break;
    case "ModifierFlags": s += `:${n.ignoreCase}:${n.multiline}:${n.dotAll}`; break;
  }
  if (n.parent) s += `^${n.parent.type}@${n.parent.start}`;
  return s;
}
function children(n) {
  switch (n.type) {
    case "Pattern": case "CapturingGroup": case "ClassStringDisjunction": return n.alternatives;
    case "Group": return (n.modifiers ? [n.modifiers] : []).concat(n.alternatives);
    case "Assertion": return n.kind === "lookahead" || n.kind === "lookbehind" ? n.alternatives : [];
    case "Alternative": case "CharacterClass": case "StringAlternative": return n.elements;
    case "Quantifier": return [n.element];
    case "CharacterClassRange": return [n.min, n.max];
    case "ClassIntersection": case "ClassSubtraction": return [n.left, n.right];
    case "ExpressionCharacterClass": return [n.expression];
    case "Modifiers": return [n.add, n.remove].filter(Boolean);
    default: return [];
  }
}
function dump(n, acc) { acc.push(ser(n)); for (const c of children(n)) dump(c, acc); return acc; }
const u16 = s => Array.from({ length: s.length }, (_, i) => s.charCodeAt(i));

const lines = [];
const validator = new RegExpValidator();
const parser = new RegExpParser();
let counts = { cases: fromCases, generated: patterns.size - fromCases, flags: flagsSet.size };
for (const p of patterns.keys()) {
  for (const [u, v] of [[false, false], [true, false], [false, true], [true, true]]) {
    let err = null, astText = null, perr = null;
    try { validator.validatePattern(p, void 0, void 0, { unicode: u, unicodeSets: v }); } catch (e) { err = e.message; }
    try { astText = dump(parser.parsePattern(p, 0, p.length, { unicode: u, unicodeSets: v }), []).join("\n"); } catch (e) { perr = e.message; }
    lines.push(JSON.stringify({ p: u16(p), u, v, err: err === null ? null : u16(err), perr: perr === null ? null : u16(perr), ast: astText }));
  }
}
for (const f of flagsSet) {
  let err = null;
  try { validator.validateFlags(f); } catch (e) { err = e.message; }
  lines.push(JSON.stringify({ f: u16(f), err: err === null ? null : u16(err) }));
}
fs.writeFileSync(out, lines.join("\n") + "\n");
console.log(JSON.stringify(counts), lines.length);
