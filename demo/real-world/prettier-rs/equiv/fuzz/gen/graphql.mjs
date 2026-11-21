// Generator aimed at GraphQL code the corpus never reaches (comment placement, string escapes,
// block strings, BOM): tokenizes the corpus GraphQL fixtures with graphql-js and, per mutant,
// applies 1-4 of: insert a `#` comment at a token boundary, replace a string token with one using
// escapes (\uXXXX, surrogate pairs, \u{...}, \/ \b \f), swap a token run with one from another
// fixture, insert a BOM or CRLF. Mutants are kept whether or not they parse.
// Usage: node graphql.mjs [--n N] [--seed S]
import { rng, loadSeeds, requireP, argValue, emit, filepathOf } from "./common.mjs";

const { Lexer, Source, TokenKind } = requireP("graphql");
const argv = process.argv.slice(2);
const N = Number(argValue(argv, "--n", 100000));
const R = rng(Number(argValue(argv, "--seed", 21)));

function tokens(text) {
  const out = [];
  try {
    const lx = new Lexer(new Source(text));
    for (let t = lx.advance(); t.kind !== TokenKind.EOF; t = lx.advance()) out.push(t);
  } catch { /* keep what was lexed */ }
  return out;
}

const seeds = [];
const runs = [];
const seen = new Set();
for (const c of loadSeeds()) {
  const fp = filepathOf(c.args);
  if (!/\.(graphql|gql|graphqls)$/i.test(fp) || seen.has(c.stdin)) continue;
  seen.add(c.stdin);
  const toks = tokens(c.stdin);
  if (!toks.length) continue;
  seeds.push({ name: c.name, fp, text: c.stdin });
  for (let i = 0; i < toks.length; i += 3) {
    const j = Math.min(toks.length - 1, i + R.int(6));
    runs.push(c.stdin.slice(toks[i].start, toks[j].end));
  }
}
console.error("seeds", seeds.length, "runs", runs.length);

const COMMENTS = ["# c\n", "#\n", "\n# own line\n", "# a\n# b\n", "  # trailing\n", "\n\n# after blank\n\n"];
const STRINGS = ['"\\u0041\\u00e9"', '"\\uD83D\\uDE00"', '"\\u{1F600}"', '"\\/\\b\\f\\n\\r\\t\\"\\\\"', '"\\uDC00"',
  '"""\n  block \\""" string\n    indented\n"""', '"""a"""', '""', '"é😀"', '"\\uD83D"', '"\\u00ZZ"', '"""\r\n  crlf\r\n"""'];

function mutate(text) {
  const toks = tokens(text);
  const op = R.int(6);
  if (op <= 1 && toks.length) {
    const t = R.pick(toks);
    const at = R.chance(0.5) ? t.start : t.end;
    return text.slice(0, at) + R.pick(COMMENTS) + text.slice(at);
  }
  if (op === 2) {
    const strs = toks.filter((t) => t.kind === TokenKind.STRING || t.kind === TokenKind.BLOCK_STRING);
    if (strs.length) { const t = R.pick(strs); return text.slice(0, t.start) + R.pick(STRINGS) + text.slice(t.end); }
    if (toks.length) { const t = R.pick(toks); return text.slice(0, t.end) + " " + R.pick(STRINGS) + " " + text.slice(t.end); }
    return null;
  }
  if (op === 3 && toks.length > 1) {
    const i = R.int(toks.length), j = Math.min(toks.length - 1, i + R.int(5));
    return text.slice(0, toks[i].start) + R.pick(runs) + text.slice(toks[j].end);
  }
  if (op === 4) return (R.chance(0.5) ? "﻿" : "") + (R.chance(0.5) ? text.replace(/\n/g, "\r\n") : text);
  if (toks.length) { const t = R.pick(toks); return text.slice(0, t.start) + text.slice(t.end); }
  return null;
}

for (let made = 0; made < N; ) {
  const s = R.pick(seeds);
  let text = s.text;
  const ops = 1 + R.int(4);
  for (let k = 0; k < ops; k++) { const m = mutate(text); if (m != null && m.length < 20000) text = m; }
  if (text === s.text) continue;
  await emit({ name: `${s.name}~g${made}`, args: ["--stdin-filepath", s.fp], stdin: text });
  made++;
}
