// node espree-dump.cjs <eslint clone> <inputs.jsonl> <out.txt>: espree's tree, tokens and comments for each input,
// in the format `linter-rs/examples/dump.rs` prints (diff the two outputs).
const espree = require(process.argv[2] + "/node_modules/espree");
const vk = require(process.argv[2] + "/node_modules/eslint-visitor-keys");
const fs = require("fs");
const lines = fs.readFileSync(process.argv[3], "utf8").split("\n").filter(Boolean);
const out = [];
for (const line of lines) {
  const c = JSON.parse(line);
  const code = (c.stdin || "").replace(/^﻿/, "").replace(/^#!([^\r\n\u2028\u2029]*)/u, (m, x) => `//${x}`);
  const cjs = c.args.includes("input.cjs");
  const jsx = c.args.some(a => a.includes("jsx"));
  let ast;
  try {
    ast = espree.parse(code, { ecmaVersion: "latest", sourceType: cjs ? "commonjs" : "module", range: true, tokens: true, comment: true, ecmaFeatures: { jsx } });
  } catch (e) { out.push(`## ${c.name}\nERROR`); continue; }
  const o = [`## ${c.name}`];
  const walk = (n, depth) => {
    const extra = n.type === "Identifier" || n.type === "PrivateIdentifier" || n.type === "JSXIdentifier" ? ` ${n.name}` : n.type === "Literal" ? ` ${n.raw}` : "";
    o.push(`${n.type} ${n.range[0]} ${n.range[1]}${extra}`);
    for (const k of vk.KEYS[n.type] || []) {
      const v = n[k];
      if (Array.isArray(v)) v.forEach(x => x && walk(x)); else if (v && typeof v.type === "string") walk(v);
    }
  };
  walk(ast);
  for (const t of ast.tokens) o.push(`T ${t.type} ${t.range[0]} ${t.range[1]} ${JSON.stringify(t.value)}`);
  if (/^#!/u.test((c.stdin || "").replace(/^\uFEFF/, "")) && ast.comments[0] && ast.comments[0].range[0] === 0) ast.comments[0].type = "Shebang";
  for (const t of ast.comments) o.push(`C ${t.type} ${t.range[0]} ${t.range[1]} ${JSON.stringify(t.value)}`);
  out.push(o.join("\n"));
}
fs.writeFileSync(process.argv[4], out.join("\n") + "\n");
