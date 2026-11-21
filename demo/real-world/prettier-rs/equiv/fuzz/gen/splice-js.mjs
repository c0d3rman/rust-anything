// Generator a (JS/TS/JSX): syntax-tree splicing. Parses the corpus fixtures with Babel, then
// builds new programs by replacing a subtree with another subtree of the same kind taken from
// any fixture of the same language (expressions, statements, TS types, JSX elements and
// attributes, class/object/interface members, parameters), and by inserting comments at token
// boundaries. Every mutant is re-parsed and dropped if it no longer parses, so programs stay valid.
// Usage: node splice-js.mjs [--n N] [--seed S] [--lang js|ts|all]
import { rng, loadSeeds, requireP, argValue, emit, filepathOf } from "./common.mjs";

const babel = requireP("@babel/parser");
const argv = process.argv.slice(2);
const N = Number(argValue(argv, "--n", 100000));
const R = rng(Number(argValue(argv, "--seed", 7)));
const langSel = argValue(argv, "--lang", "all");

const GROUPS = {
  js: { re: /\.(js|mjs|cjs)$/i, plugins: ["jsx"] },
  jsx: { re: /\.jsx$/i, plugins: ["jsx"] },
  ts: { re: /\.(ts|mts|cts)$/i, plugins: ["typescript"] },
  tsx: { re: /\.tsx$/i, plugins: ["typescript", "jsx"] },
};
// donor pools shared across these
const POOL_OF = { js: "js", jsx: "js", ts: "ts", tsx: "ts" };

function parse(text, plugins) {
  return babel.parse(text, {
    sourceType: "module",
    allowReturnOutsideFunction: true,
    allowAwaitOutsideFunction: true,
    allowImportExportEverywhere: true,
    allowSuperOutsideMethod: true,
    allowUndeclaredExports: true,
    errorRecovery: false,
    tokens: true,
    plugins: [...plugins, "decorators-legacy"],
  });
}

const EXPR = new Set(["CallExpression", "NewExpression", "MemberExpression", "OptionalMemberExpression",
  "OptionalCallExpression", "BinaryExpression", "LogicalExpression", "ConditionalExpression",
  "ArrowFunctionExpression", "FunctionExpression", "ObjectExpression", "ArrayExpression", "TemplateLiteral",
  "TaggedTemplateExpression", "AssignmentExpression", "UnaryExpression", "UpdateExpression", "AwaitExpression",
  "SequenceExpression", "StringLiteral", "NumericLiteral", "BigIntLiteral", "BooleanLiteral", "NullLiteral",
  "RegExpLiteral", "ClassExpression", "JSXElement", "JSXFragment", "TSAsExpression", "TSSatisfiesExpression",
  "TSNonNullExpression", "TSTypeAssertion", "ThisExpression", "Identifier", "ImportExpression"]);
const NOT_EXPR_KEYS = new Set(["key", "property", "label", "id", "params", "local", "exported", "imported",
  "source", "meta", "typeName", "parameterName", "name", "left"]);
const STMT_KEYS = new Set(["body", "consequent", "alternate"]);
const MEMBER = {
  ClassMethod: "class", ClassProperty: "class", ClassPrivateMethod: "class", ClassPrivateProperty: "class",
  ClassAccessorProperty: "class", StaticBlock: "class", TSDeclareMethod: "class", TSIndexSignature: "class",
  ObjectProperty: "object", ObjectMethod: "object", SpreadElement: "object",
  TSPropertySignature: "iface", TSMethodSignature: "iface", TSCallSignatureDeclaration: "iface",
  TSConstructSignatureDeclaration: "iface",
};

// Returns [{kind, start, end}] for every subtree that can be swapped.
function sites(ast) {
  const out = [];
  const visit = (node, parent, key) => {
    if (!node || typeof node.type !== "string") return;
    const t = node.type;
    let kind = null;
    if (EXPR.has(t) && !NOT_EXPR_KEYS.has(key) && !(t === "Identifier" && parent && parent.type?.startsWith("TS"))) {
      if (!(parent && (parent.type === "ObjectProperty" || parent.type === "ClassProperty") && parent.shorthand)) kind = "expr";
    } else if ((t.endsWith("Statement") || t.endsWith("Declaration")) && STMT_KEYS.has(key) && !t.startsWith("Export") && !t.startsWith("Import")) {
      kind = "stmt";
    } else if (t.startsWith("TS") && (t.endsWith("Type") || t.endsWith("Keyword") || t === "TSTypeReference")) {
      kind = "type";
    } else if (t === "JSXElement" || t === "JSXFragment" || t === "JSXText" || t === "JSXExpressionContainer") {
      if (parent && (parent.type === "JSXElement" || parent.type === "JSXFragment")) kind = "jsxchild";
    } else if (t === "JSXAttribute" || t === "JSXSpreadAttribute") {
      kind = "jsxattr";
    } else if (MEMBER[t]) {
      kind = MEMBER[t];
    } else if ((key === "params") && (t === "Identifier" || t === "AssignmentPattern" || t === "ObjectPattern" || t === "ArrayPattern" || t === "RestElement" || t === "TSParameterProperty")) {
      kind = "param";
    }
    if (kind && node.start != null && node.end > node.start) out.push({ kind, start: node.start, end: node.end, type: t });
    for (const k of Object.keys(node)) {
      if (k === "loc" || k === "extra" || k === "leadingComments" || k === "trailingComments" || k === "innerComments") continue;
      const v = node[k];
      if (Array.isArray(v)) for (const c of v) visit(c, node, k);
      else if (v && typeof v === "object" && typeof v.type === "string") visit(v, node, k);
    }
  };
  visit(ast.program, null, null);
  return out;
}

// ---- load seeds and build donor pools
const seeds = [];
const pools = { js: {}, ts: {} };
const seen = new Set();
for (const c of loadSeeds()) {
  const fp = filepathOf(c.args);
  const lang = Object.keys(GROUPS).find((g) => GROUPS[g].re.test(fp));
  if (!lang || (langSel !== "all" && POOL_OF[lang] !== langSel)) continue;
  if (c.stdin.length > 20000 || seen.has(fp + "\0" + c.stdin)) continue;
  seen.add(fp + "\0" + c.stdin);
  let ast;
  try { ast = parse(c.stdin, GROUPS[lang].plugins); } catch { continue; }
  seeds.push({ name: c.name, fp, lang, text: c.stdin });
  const pool = pools[POOL_OF[lang]];
  for (const s of sites(ast)) {
    const snippet = c.stdin.slice(s.start, s.end);
    if (snippet.length > 600) continue;
    const key = s.kind + "\0" + snippet;
    if (seen.has(key)) continue;
    seen.add(key);
    (pool[s.kind] ??= []).push(snippet);
  }
}
for (const [p, kinds] of Object.entries(pools))
  console.error(p, Object.fromEntries(Object.entries(kinds).map(([k, v]) => [k, v.length])));
console.error("seeds", seeds.length);

const COMMENTS = ["/* c */", "// c\n", "/** doc */", "\n// line\n", "/* a */ /* b */", "\n\n/* block\n   comment */\n",
  "// prettier-ignore\n", "/*: any */", "/* long comment that goes on for quite a while to push things past the width */"];

function mutateOnce(text, ast, lang) {
  const pool = pools[POOL_OF[lang]];
  if (R.chance(0.2)) {
    // comment placement: insert at a token boundary
    const toks = ast.tokens ?? [];
    if (toks.length === 0) return null;
    const t = R.pick(toks);
    const at = R.chance(0.5) ? t.start : t.end;
    return text.slice(0, at) + (R.chance(0.5) ? " " : "") + R.pick(COMMENTS) + text.slice(at);
  }
  const ss = sites(ast);
  if (ss.length === 0) return null;
  const s = R.pick(ss);
  const donors = pool[s.kind];
  if (!donors || donors.length === 0) return null;
  let d = R.pick(donors);
  if (s.kind === "expr" && R.chance(0.25)) d = `(${d})`;
  return text.slice(0, s.start) + d + text.slice(s.end);
}

let made = 0;
let tries = 0;
while (made < N) {
  tries++;
  const seed = R.pick(seeds);
  let text = seed.text;
  let ast = parse(text, GROUPS[seed.lang].plugins);
  const ops = 1 + R.int(4);
  let applied = 0;
  for (let k = 0; k < ops; k++) {
    const next = mutateOnce(text, ast, seed.lang);
    if (next == null || next.length > 30000) continue;
    try {
      ast = parse(next, GROUPS[seed.lang].plugins);
      text = next;
      applied++;
    } catch { /* dropped: no longer parses */ }
  }
  if (applied === 0) continue;
  await emit({ name: `${seed.name}~s${made}`, args: ["--stdin-filepath", seed.fp], stdin: text });
  made++;
}
console.error("made", made, "tries", tries);
