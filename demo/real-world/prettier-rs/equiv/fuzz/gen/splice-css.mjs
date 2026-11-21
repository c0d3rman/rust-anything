// Generator a (CSS/SCSS/Less): splicing at the rule, declaration and value level. Parses the
// corpus style fixtures with the PostCSS parsers Prettier uses, then replaces a node with a
// donor of the same kind from any fixture of the same language: whole rules/at-rules,
// declarations, comments, declaration values, selectors, and at-rule params (same at-rule
// name). PostCSS keeps raws, so untouched text is reproduced exactly. Mutants that no longer
// parse are dropped. Usage: node splice-css.mjs [--n N] [--seed S] [--lang css|scss|less|all]
import { rng, loadSeeds, requireP, argValue, emit, filepathOf } from "./common.mjs";

const postcss = requireP("postcss");
const SYNTAX = { css: postcss, scss: requireP("postcss-scss"), less: requireP("postcss-less") };
const argv = process.argv.slice(2);
const N = Number(argValue(argv, "--n", 100000));
const R = rng(Number(argValue(argv, "--seed", 11)));
const langSel = argValue(argv, "--lang", "all");

const parse = (text, lang) => SYNTAX[lang].parse(text);
const langOf = (fp) => (/\.scss$/i.test(fp) ? "scss" : /\.less$/i.test(fp) ? "less" : /\.css$/i.test(fp) ? "css" : null);

const seeds = [];
const pools = { css: {}, scss: {}, less: {} };
const seen = new Set();
const add = (lang, kind, v) => {
  if (typeof v !== "string" || v.length > 500) return;
  const key = lang + kind + "\0" + v;
  if (seen.has(key)) return;
  seen.add(key);
  (pools[lang][kind] ??= []).push(v);
};
for (const c of loadSeeds()) {
  const fp = filepathOf(c.args);
  const lang = langOf(fp);
  if (!lang || (langSel !== "all" && lang !== langSel) || c.args.includes("--parser")) continue;
  if (seen.has(fp + "\0" + c.stdin) || c.stdin.length > 20000) continue;
  seen.add(fp + "\0" + c.stdin);
  let root;
  try { root = parse(c.stdin, lang); } catch { continue; }
  seeds.push({ name: c.name, fp, lang, text: c.stdin });
  root.walk((n) => {
    const s = n.toString();
    if (n.type === "decl") { add(lang, "decl", s); add(lang, "value", n.raws.value?.raw ?? n.value); }
    else if (n.type === "rule") { add(lang, "node", s); add(lang, "selector", n.raws.selector?.raw ?? n.selector); }
    else if (n.type === "atrule") { add(lang, "node", s); add(lang, "params:" + n.name, n.raws.params?.raw ?? n.params); }
    else if (n.type === "comment") add(lang, "comment", s);
  });
}
console.error("seeds", seeds.length, Object.fromEntries(Object.entries(pools).map(([l, p]) => [l, Object.fromEntries(Object.entries(p).filter(([k]) => !k.startsWith("params:")).map(([k, v]) => [k, v.length]))])));

// Parses a donor snippet into nodes (for node/decl/comment kinds), or null.
function nodesOf(snippet, lang, kind) {
  try {
    const wrapped = kind === "decl" ? `a{${snippet}}` : snippet;
    const root = parse(wrapped, lang);
    const nodes = kind === "decl" ? root.first.nodes : root.nodes;
    return nodes.length ? nodes.map((n) => n.clone()) : null;
  } catch { return null; }
}

function mutateOnce(root, lang) {
  const all = [];
  root.walk((n) => all.push(n));
  if (all.length === 0) return false;
  const n = R.pick(all);
  const pool = pools[lang];
  const op = R.int(4);
  if (op === 0 || n.type === "comment") {
    // replace the node with a donor of its kind
    const kind = n.type === "decl" ? "decl" : n.type === "comment" ? "comment" : "node";
    const donors = pool[kind];
    if (!donors) return false;
    const nodes = nodesOf(R.pick(donors), lang, kind);
    if (!nodes) return false;
    for (const m of nodes) m.raws.before = n.raws.before;
    n.replaceWith(...nodes);
    return true;
  }
  if (op === 1) {
    // insert a donor rule/decl/comment before this node
    const kind = n.parent?.type === "rule" ? R.pick(["decl", "decl", "comment", "node"]) : R.pick(["node", "comment"]);
    const donors = pool[kind];
    if (!donors) return false;
    const nodes = nodesOf(R.pick(donors), lang, kind);
    if (!nodes) return false;
    for (const m of nodes) m.raws.before = n.raws.before ?? "\n";
    n.before(nodes);
    return true;
  }
  if (n.type === "decl") {
    const v = R.pick(pool.value ?? [""]);
    n.value = v;
    if (n.raws.value) delete n.raws.value;
    return true;
  }
  if (n.type === "rule") {
    const s = R.pick(pool.selector ?? [""]);
    n.selector = s;
    if (n.raws.selector) delete n.raws.selector;
    return true;
  }
  if (n.type === "atrule") {
    const donors = pool["params:" + n.name];
    if (!donors) return false;
    n.params = R.pick(donors);
    if (n.raws.params) delete n.raws.params;
    return true;
  }
  return false;
}

let made = 0;
let tries = 0;
while (made < N) {
  tries++;
  const seed = R.pick(seeds);
  let text = seed.text;
  const ops = 1 + R.int(4);
  let applied = 0;
  for (let k = 0; k < ops; k++) {
    let next;
    try {
      const root = parse(text, seed.lang);
      if (!mutateOnce(root, seed.lang)) continue;
      next = root.toString(SYNTAX[seed.lang].stringify ?? undefined);
      parse(next, seed.lang);
    } catch { continue; }
    if (next.length > 30000) continue;
    text = next;
    applied++;
  }
  if (applied === 0 || text === seed.text) continue;
  await emit({ name: `${seed.name}~c${made}`, args: ["--stdin-filepath", seed.fp], stdin: text });
  made++;
}
console.error("made", made, "tries", tries);
