// Inputs aimed at every option-dependent Doc construction site (README, "Which widths the sweep
// uses"), with lengths and tab settings on both sides of each threshold. Writes probes.jsonl in
// the corpus format; run it through `driver.mjs sweep` like the corpus.
//   node gen-probes.mjs > out/probes.jsonl
const out = [];
const add = (name, file, stdin, flags = []) => out.push({ name, args: ["--stdin-filepath", file, ...flags], stdin });
const id = (n, c = "a") => c.repeat(n);
const tabs = [[], ["--tab-width", "1"], ["--tab-width", "3"], ["--tab-width", "4"], ["--tab-width", "5"], ["--tab-width", "8"], ["--use-tabs"], ["--use-tabs", "--tab-width", "4"]];
const tname = (t) => (t.length ? t.join("").replaceAll("-", "") : "default");

// #1/#1a isLoneShortArgument through the assignment "poorly breakable chain" layout.
for (let n = 1; n <= 56; n++) {
  const lhs = "const someVariableNameThatIsLong";
  add(`p1-ident-${n}`, "p.js", `${lhs} = this.someObject.someMethodName(${id(n)});\n${lhs} = someObject.method(${id(n)}).other(${id(n)});\n`);
  add(`p1-string-${n}`, "p.js", `${lhs} = this.someObject.someMethodName("${id(n - 1 || 1, "s")}");\nx = a.b('${id(n, "q")}');\n`, ["--single-quote"]);
  add(`p1-regex-${n}`, "p.js", `${lhs} = this.someObject.someMethodName(/${id(n, "r")}/g);\n`);
  add(`p1-template-${n}`, "p.js", `${lhs} = this.someObject.someMethodName(\`${id(n, "t")}\`);\n`);
  add(`p1-call0-${n}`, "p.js", `${lhs} = this.someObject.someMethodName(${id(n, "f")}());\n`);
  add(`p1-unary-${n}`, "p.ts", `${lhs} = this.someObject.someMethodName(!${id(n, "u")});\n${lhs} = this.someObject.someMethodName(-${id(n, "v")});\n`);
  add(`p1b-ternary-${n}`, "p.js", `const message = someCondition ? ${id(n, "c")} : someOtherCondition ? anotherValue : yetAnotherValueThatIsLong;\n`, ["--experimental-ternaries"]);
  add(`p1-md-${n}`, "p.md", "# t\n\n```js\n" + `${lhs} = this.someObject.someMethodName(${id(n)});\n` + "```\n");
}
// #2 short object key (getStringWidth(key) < tabWidth + 3), #3 member-chain short head (<= tabWidth).
for (const t of tabs)
  for (let n = 1; n <= 12; n++) {
    add(`p2-key-${tname(t)}-${n}`, "p.js", `const o = {\n  ${id(n, "k")}: "a very long string value that does not fit on the line at all, really not",\n  ${id(n, "k")}: someFunctionCall(argumentNumberOne) || someOtherFunctionCall(argumentNumberTwo),\n};\n`, t);
    add(`p3-chain-${tname(t)}-${n}`, "p.js", `${id(n, "z")}.someMethod().anotherMethod().yetAnotherMethodName().finalMethodCall();\n${id(n, "z")}.a().b().c();\n`, t);
  }
// #4-#6 ternaries (default and experimental) under tab settings.
const tern = `const animationName = someCondition ? (anotherCondition ? valueNumberOne : valueNumberTwo) : yetAnotherCondition ? valueNumberThree : valueNumberFour;\nconst x = a ? b : c ? d : e ? f : someVeryLongIdentifierName + anotherVeryLongIdentifierName + more;\n`;
for (const t of tabs) {
  add(`p5-ternary-${tname(t)}`, "p.js", tern, t);
  add(`p6-xternary-${tname(t)}`, "p.js", tern, [...t, "--experimental-ternaries"]);
}
// #7/#8 template literal indentation, #9 interpolations, #10 jest each tables.
const tpl = [
  "const a = `\n\t  line ${foo(bar, baz)}\n  \t${{ a: 1, b: [1, 2, 3] }}\n`;\n",
  "const b = `\n\t\t\t${someFunction(argumentOne, argumentTwo, argumentThree, argumentFour, five)}\n`;\n",
  "const c = `x\n   \t ${a ? b : c}`;\n",
  "describe.each`\n  a    | b    | expected\n  ${1} | ${1} | ${2}\n  ${{ a: 1, b: 2 }} | ${[1, 2, 3]} | ${() => { return 1; }}\n`('x', () => {});\n",
];
for (const t of tabs) tpl.forEach((s, i) => add(`p7-template-${i}-${tname(t)}`, "p.js", s, t));
// #11-#13 markdown list alignment, #14/#15 tables, #16 embedded code.
const md = [
  "1. one\n\n   para\n\n2. two\n",
  "1.  one\n2.  two\n",
  "10. ten\n\n    para\n",
  "- a\n\n      indented code\n",
  "1. a\n\n        code\n",
  "3. x\n   ```js\n   const a = {b:1}\n   ```\n",
  "| a | b |\n|---|:-:|\n| `some code` | longer cell text here and more |\n| x | y |\n",
  "* [ ] task\n\n  * nested\n",
];
for (const t of tabs)
  for (const pw of [[], ["--prose-wrap", "always"], ["--prose-wrap", "never"]])
    md.forEach((s, i) => add(`p11-md-${i}-${tname(t)}-${tname(pw)}`, "p.md", s, [...t, ...pw]));
// #17-#19 YAML aligns.
const yaml = [
  "a: |\n  text\n  more\nb: >\n  folded\n  text\n",
  "key: {a: 1, b: [1, 2, 3], c: {d: e}}\nlist: [one, two, three, four, five, six, seven, eight, nine, ten, eleven]\n",
  "? complex key\n: value\nsome long key name: some very long value that will need to wrap at some point in time\n",
  "- a: |2\n    indented\n",
];
for (const t of tabs) yaml.forEach((s, i) => add(`p17-yaml-${i}-${tname(t)}`, "p.yaml", s, t.filter((f) => f !== "--use-tabs")));
for (const c of out) console.log(JSON.stringify(c));
