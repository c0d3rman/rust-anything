// Long-lived Prettier oracle. Reads JSON lines {id, args, text} on stdin, writes
// {id, ok, out} or {id, ok:false, err} on stdout. `args` are Prettier CLI flags as in the
// corpus; they are turned into the options object the CLI would pass to `prettier.format`.
import path from "node:path";
import { pathToFileURL } from "node:url";

const dir = process.env.PRETTIER_DIST;
if (!dir) {
  console.error("Set PRETTIER_DIST to Prettier 3.6.2's built dist/prettier directory.");
  process.exit(2);
}
const prettier = await import(pathToFileURL(path.join(dir, "index.mjs")).href);

const camel = (s) => s.replace(/-([a-z])/g, (_, c) => c.toUpperCase());
const VALUE = new Set(["stdin-filepath", "print-width", "tab-width", "trailing-comma", "arrow-parens",
  "quote-props", "object-wrap", "experimental-operator-position", "end-of-line", "parser", "prose-wrap"]);
const NUMERIC = new Set(["print-width", "tab-width"]);

export function argsToOptions(args) {
  const o = {};
  for (let i = 0; i < args.length; i++) {
    let a = args[i];
    if (!a.startsWith("--")) throw new Error("bad arg " + a);
    a = a.slice(2);
    let v;
    const eq = a.indexOf("=");
    if (eq >= 0) { v = a.slice(eq + 1); a = a.slice(0, eq); }
    if (VALUE.has(a)) {
      if (v === undefined) v = args[++i];
      if (a === "stdin-filepath") o.filepath = v;
      else o[camel(a)] = NUMERIC.has(a) ? Number(v) : v;
    } else if (a.startsWith("no-")) o[camel(a.slice(3))] = false;
    else o[camel(a)] = true;
  }
  return o;
}

// Split on "\n" only: readline also splits on U+2028/U+2029, which JSON leaves unescaped.
async function* lines() {
  let buf = "";
  process.stdin.setEncoding("utf8");
  for await (const chunk of process.stdin) {
    buf += chunk;
    let i;
    while ((i = buf.indexOf("\n")) >= 0) { yield buf.slice(0, i); buf = buf.slice(i + 1); }
  }
  if (buf) yield buf;
}
for await (const line of lines()) {
  if (!line) continue;
  const req = JSON.parse(line);
  let res;
  try {
    const out = await prettier.format(req.text, argsToOptions(req.args));
    // Prettier slices by UTF-16 unit and can split a surrogate pair; Rust strings cannot hold a
    // lone surrogate, so send U+FFFD in its place and flag it.
    res = out.isWellFormed() ? { id: req.id, ok: true, out } : { id: req.id, ok: true, out: out.toWellFormed(), loneSurrogate: true };
  } catch (e) {
    res = { id: req.id, ok: false, err: String(e?.message ?? e).slice(0, 300) };
  }
  process.stdout.write(JSON.stringify(res) + "\n");
}
