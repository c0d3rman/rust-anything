// Generator b: random option combinations. Reads cases (JSONL, or the seed corpus by default),
// drops their formatting flags, and emits each with random values for every option the port
// reads. Usage: node options.mjs [--in FILE|-] [--per K] [--seed S] [--max N]
import fs from "node:fs";
import { rng, SEEDS, argValue, emit, stdinLines } from "./common.mjs";

const argv = process.argv.slice(2);
const per = Number(argValue(argv, "--per", 4));
const max = Number(argValue(argv, "--max", Infinity));
const R = rng(Number(argValue(argv, "--seed", 1)));
const input = argValue(argv, "--in", SEEDS);

const WIDTHS = [0, 1, 2, 3, 5, 8, 10, 15, 20, 25, 30, 40, 50, 60, 70, 80, 100, 120, 200, 1000];
const BOOLS = ["--use-tabs", "--no-semi", "--single-quote", "--jsx-single-quote", "--no-bracket-spacing",
  "--bracket-same-line", "--experimental-ternaries"];
const ENUMS = {
  "--quote-props": ["as-needed", "consistent", "preserve"],
  "--trailing-comma": ["all", "es5", "none"],
  "--object-wrap": ["preserve", "collapse"],
  "--arrow-parens": ["always", "avoid"],
  "--prose-wrap": ["always", "never", "preserve"],
  "--end-of-line": ["lf", "crlf", "cr", "auto"],
  "--experimental-operator-position": ["start", "end"],
};
const STYLE = /\.(css|scss|less|pcss|postcss|wxss)$/i;

export function randomOptions(R, filepath, parser) {
  const args = ["--stdin-filepath", filepath];
  if (parser) args.push("--parser", parser);
  else if (STYLE.test(filepath) && R.chance(0.1)) args.push("--parser", R.pick(["css", "scss", "less"]));
  if (R.chance(0.7)) args.push("--print-width", String(R.pick(WIDTHS)));
  if (R.chance(0.4)) args.push("--tab-width", String(R.pick([0, 1, 2, 3, 4, 8])));
  for (const b of BOOLS) if (R.chance(0.3)) args.push(b);
  for (const [k, vs] of Object.entries(ENUMS)) if (R.chance(k === "--end-of-line" ? 0.15 : 0.4)) args.push(k, R.pick(vs));
  return args;
}

function crlf(text) { return text.replace(/\r?\n/g, "\r\n"); }

const lines = input === "-" ? stdinLines()
  : fs.readFileSync(input, "utf8").split("\n");
let n = 0;
outer: for await (const line of lines) {
  if (!line) continue;
  const c = JSON.parse(line);
  const fp = c.args[c.args.indexOf("--stdin-filepath") + 1];
  const pi = c.args.indexOf("--parser");
  const parser = pi >= 0 ? c.args[pi + 1] : undefined;
  for (let k = 0; k < per; k++) {
    if (n++ >= max) break outer;
    const stdin = R.chance(0.05) ? crlf(c.stdin) : c.stdin;
    await emit({ name: `${c.name}~o${k}`, args: randomOptions(R, fp, parser), stdin });
  }
}
