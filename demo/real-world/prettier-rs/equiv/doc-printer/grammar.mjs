// Doc enumeration over a small alphabet. Docs are produced as JSON strings in the port's wire
// format (the format `scripts/harvest-docs.mjs` writes); `materialize` turns one into the JS
// object Prettier is given.
//
// Size = number of alphabet symbols in the doc: every leaf, unary, binary and list constructor
// counts 1 (a list constructor counts 1 plus its items), so `fill([])` is size 1 and
// `group(["a", line])` is size 4 (group + array + "a" + line).

const J = JSON.stringify;
const LINE = '{"type":"line"}';
const SOFT = '{"type":"line","soft":true}';
const HARD_RAW = '{"type":"line","hard":true}';
const LIT_RAW = '{"type":"line","hard":true,"literal":true}';
const BP = '{"type":"break-parent"}';

export const ALPHABETS = {
  // Every doc command the Rust `Doc` has.
  full: {
    leaves: [
      J(""), J("a"), J(" "), J("\t"), J("字"),
      LINE, SOFT, HARD_RAW, LIT_RAW,
      `[${HARD_RAW},${BP}]`, // hardline (builder macro)
      `[${LIT_RAW},${BP}]`, // literalline (builder macro)
      BP, '{"type":"trim"}', '{"type":"cursor"}', '{"type":"line-suffix-boundary"}',
    ],
    unary: [
      c => `{"type":"indent","contents":${c}}`,
      c => `{"type":"align","n":2,"contents":${c}}`,
      c => `{"type":"align","n":-1,"contents":${c}}`,
      c => `{"type":"align","n":"> ","contents":${c}}`,
      c => `{"type":"align","n":{"$num":"-Infinity"},"contents":${c}}`, // dedentToRoot
      c => `{"type":"align","n":{"type":"root"},"contents":${c}}`, // markAsRoot
      c => `{"type":"group","contents":${c},"break":false}`,
      c => `{"type":"group","contents":${c},"break":true}`,
      c => `{"type":"group","id":"A","contents":${c},"break":false}`,
      c => `{"type":"group","id":"B","contents":${c},"break":false}`,
      c => `{"type":"group","id":"A","contents":${c},"break":true}`,
      c => `{"type":"group","id":"B","contents":${c},"break":true}`,
      c => `{"type":"line-suffix","contents":${c}}`,
      c => `{"type":"label","label":"L","contents":${c}}`,
      c => `{"type":"indent-if-break","groupId":"A","contents":${c},"negate":false}`,
      c => `{"type":"indent-if-break","groupId":"A","contents":${c},"negate":true}`,
      c => `{"type":"indent-if-break","groupId":"B","contents":${c},"negate":false}`,
      c => `{"type":"indent-if-break","groupId":"B","contents":${c},"negate":true}`,
      c => `{"type":"indent-if-break","contents":${c},"negate":false}`,
      c => `{"type":"indent-if-break","contents":${c},"negate":true}`,
    ],
    binary: [
      (a, b) => `{"type":"if-break","breakContents":${a},"flatContents":${b}}`,
      (a, b) => `{"type":"if-break","groupId":"A","breakContents":${a},"flatContents":${b}}`,
      (a, b) => `{"type":"if-break","groupId":"B","breakContents":${a},"flatContents":${b}}`,
    ],
    lists: [
      { min: 2, make: xs => `[${xs.join(",")}]` }, // concat
      { min: 0, make: xs => `{"type":"fill","parts":[${xs.join(",")}]}` },
      // conditionalGroup(states, opts): contents is states[0] (same object on the JS side)
      { min: 1, make: xs => `{"type":"group","cg":1,"contents":${xs[0]},"break":false,"expandedStates":[${xs.join(",")}]}` },
      { min: 1, make: xs => `{"type":"group","cg":1,"contents":${xs[0]},"break":true,"expandedStates":[${xs.join(",")}]}` },
      { min: 1, make: xs => `{"type":"group","cg":1,"id":"A","contents":${xs[0]},"break":false,"expandedStates":[${xs.join(",")}]}` },
    ],
  },
  // A smaller alphabet that reaches deeper sizes: one representative per printer code path
  // that matters most for layout decisions (width, fits, group modes, fill, indentation).
  small: {
    leaves: [J("a"), J("字"), J(" "), LINE, SOFT, `[${HARD_RAW},${BP}]`, '{"type":"trim"}', '{"type":"line-suffix-boundary"}'],
    unary: [
      c => `{"type":"indent","contents":${c}}`,
      c => `{"type":"align","n":{"$num":"-Infinity"},"contents":${c}}`,
      c => `{"type":"group","contents":${c},"break":false}`,
      c => `{"type":"group","id":"A","contents":${c},"break":false}`,
      c => `{"type":"line-suffix","contents":${c}}`,
      c => `{"type":"indent-if-break","groupId":"A","contents":${c},"negate":false}`,
    ],
    binary: [
      (a, b) => `{"type":"if-break","groupId":"A","breakContents":${a},"flatContents":${b}}`,
    ],
    lists: [
      { min: 2, make: xs => `[${xs.join(",")}]` },
      { min: 0, make: xs => `{"type":"fill","parts":[${xs.join(",")}]}` },
      { min: 1, make: xs => `{"type":"group","cg":1,"contents":${xs[0]},"break":false,"expandedStates":[${xs.join(",")}]}` },
    ],
  },
  // Inputs the printer accepts but Prettier's builders never produce: a group whose
  // expandedStates differ from its contents (including an empty list), invalid docs, odd
  // align widths, null children.
  exotic: {
    leaves: [J("a"), J(""), LINE, `[${HARD_RAW},${BP}]`, BP, '{"type":"cursor"}', "[]", '{"type":"bogus"}', "null", "1"],
    unary: [
      c => `{"type":"group","contents":${c},"break":false}`,
      c => `{"type":"group","id":"A","contents":${c},"break":false}`,
      c => `{"type":"group","contents":${c},"break":false,"expandedStates":[]}`,
      c => `{"type":"group","id":"A","contents":${c},"break":false,"expandedStates":[]}`,
      c => `{"type":"group","contents":${c},"break":true,"expandedStates":[]}`,
      c => `{"type":"align","n":0,"contents":${c}}`,
      c => `{"type":"align","n":1.5,"contents":${c}}`,
      c => `{"type":"align","n":"","contents":${c}}`,
      c => `{"type":"indent","contents":${c}}`,
    ],
    binary: [
      (a, b) => `{"type":"group","contents":${a},"break":false,"expandedStates":[${b}]}`,
      (a, b) => `{"type":"group","contents":${a},"break":false,"expandedStates":[${a},${b}]}`, // states[0] a copy, not the same object
      (a, b) => `{"type":"if-break","groupId":"A","breakContents":${a},"flatContents":${b}}`,
    ],
    lists: [
      { min: 2, make: xs => `[${xs.join(",")}]` },
      { min: 0, make: xs => `{"type":"fill","parts":[${xs.join(",")}]}` },
    ],
  },
};

export function makeEnumerator(alpha) {
  const memo = [[]];
  function* seqs(total, min) {
    // ordered sequences of docs whose sizes sum to `total`, at least `min` long
    if (total === 0) { if (min <= 0) yield []; return; }
    for (let first = 1; first <= total; first++) {
      const firsts = docsOf(first);
      for (const rest of seqs(total - first, min - 1)) for (const d of firsts) yield [d, ...rest];
    }
  }
  function* gen(n) {
    if (n === 1) {
      yield* alpha.leaves;
      for (const l of alpha.lists) if (l.min === 0) yield l.make([]);
      return;
    }
    for (const u of alpha.unary) for (const c of docsOf(n - 1)) yield u(c);
    for (let k = 1; k <= n - 2; k++) for (const b of alpha.binary) for (const x of docsOf(k)) for (const y of docsOf(n - 1 - k)) yield b(x, y);
    for (const l of alpha.lists) for (const xs of seqs(n - 1, Math.max(l.min, 1))) yield l.make(xs);
  }
  function docsOf(n) {
    if (!memo[n]) memo[n] = [...gen(n)];
    return memo[n];
  }
  return { gen, docsOf };
}

// Group-id symmetry: renaming ids is invisible to both printers (ids are only compared for
// equality), so keep only docs whose first id in preorder is "A". The JSON is written with
// `id`/`groupId` before the children, so text order is preorder.
const ID_RE = /"(?:id|groupId)":"([AB])"/;
export const canonical = s => { const m = ID_RE.exec(s); return !m || m[1] === "A"; };

// The JS object Prettier sees: `{$num}` markers decoded, and a conditional group's contents
// made the same object as its first state (as `conditionalGroup` builds it).
export function materialize(s) {
  const doc = JSON.parse(s);
  if (s.includes("$num") || s.includes('"cg"')) fix(doc);
  return doc;
}
function fix(d) {
  if (Array.isArray(d)) { d.forEach(fix); return; }
  if (!d || typeof d !== "object") return;
  if (d.n && typeof d.n === "object" && "$num" in d.n) d.n = Number(d.n.$num);
  for (const k of ["contents", "breakContents", "flatContents"]) if (k in d) fix(d[k]);
  if (d.parts) fix(d.parts);
  if (d.expandedStates) fix(d.expandedStates);
  if (d.cg) { d.contents = d.expandedStates[0]; delete d.cg; }
}

export const hasIndentation = s => s.includes('"indent') || s.includes('"align"');
