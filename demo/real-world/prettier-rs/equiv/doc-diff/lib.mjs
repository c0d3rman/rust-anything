// Shared pieces of the Doc-level differential check: CLI flags -> Prettier options, Prettier's
// propagateBreaks, rebuilding the port's doc from its wire form, and the canonicaliser that both
// docs go through before they are compared. See README.md for what each normalisation is and why
// it cannot change printed output.
import { spawn } from "node:child_process";
import path from "node:path";

// Reads a required environment variable, or stops with a message saying what it must point to.
export function requireEnv(name, what) {
  const v = process.env[name];
  if (!v) {
    console.error(`doc-diff: set ${name} to ${what} (see README.md)`);
    process.exit(2);
  }
  return v;
}

export const PRETTIER_ROOT = requireEnv("PRETTIER_ROOT", "a clone of Prettier 3.6.2 built with `yarn build`");
export const TARGET_DIR = requireEnv("CARGO_TARGET_DIR", "the target directory rs/ was built into");
export const prettier = await import(path.join(PRETTIER_ROOT, "dist/prettier/index.mjs"));
const getStringWidth = prettier.util.getStringWidth;

export function flagsToOptions(args) {
  const o = {};
  for (let i = 0; i < args.length; i++) {
    let a = args[i];
    let inline;
    if (a.startsWith("--") && a.includes("=")) [a, inline] = [a.slice(0, a.indexOf("=")), a.slice(a.indexOf("=") + 1)];
    const next = () => inline ?? args[++i];
    switch (a) {
      case "--stdin-filepath": o.filepath = next(); break;
      case "--parser": o.parser = next(); break;
      case "--prose-wrap": o.proseWrap = next(); break;
      case "--end-of-line": o.endOfLine = next(); break;
      case "--no-semi": o.semi = false; break;
      case "--single-quote": o.singleQuote = true; break;
      case "--jsx-single-quote": o.jsxSingleQuote = true; break;
      case "--trailing-comma": o.trailingComma = next(); break;
      case "--arrow-parens": o.arrowParens = next(); break;
      case "--print-width": o.printWidth = Number(next()); break;
      case "--use-tabs": o.useTabs = true; break;
      case "--tab-width": o.tabWidth = Number(next()); break;
      case "--no-bracket-spacing": o.bracketSpacing = false; break;
      case "--quote-props": o.quoteProps = next(); break;
      case "--bracket-same-line": o.bracketSameLine = true; break;
      case "--object-wrap": o.objectWrap = next(); break;
      case "--experimental-ternaries": o.experimentalTernaries = true; break;
      case "--experimental-operator-position": o.experimentalOperatorPosition = next(); break;
      default: throw new Error("unknown flag " + a);
    }
  }
  return o;
}

export function language(args) {
  const o = flagsToOptions(args);
  if (o.parser) return o.parser;
  const f = (o.filepath ?? "").toLowerCase();
  const ext = f.includes(".") ? f.slice(f.lastIndexOf(".") + 1) : path.basename(f);
  if (["js", "cjs", "mjs", "jsx"].includes(ext)) return "js";
  if (["ts", "cts", "mts", "tsx"].includes(ext)) return "ts";
  if (["json", "jsonc", "json5"].includes(ext) || path.basename(f) === ".prettierrc") return "json";
  if (["graphql", "gql", "graphqls"].includes(ext)) return "graphql";
  if (["md", "markdown", "mdown", "mkdn", "mdx", "lr", "readme"].includes(ext)) return "markdown";
  if (["css", "scss", "less"].includes(ext)) return ext;
  return "yaml";
}

// Prettier's text preparation (normalizeInputAndOptions): drop a BOM, CRLF/CR -> LF.
export function prepareText(text) {
  if (text.charCodeAt(0) === 0xfeff) text = text.slice(1);
  return text.includes("\r") ? text.replaceAll(/\r\n?/g, "\n") : text;
}

// src/document/utils.js propagateBreaks, verbatim in behaviour (visited set on groups,
// conditional groups never broken by propagation).
export function propagateBreaks(doc) {
  const visited = new Set();
  const stack = [];
  const breakParent = () => {
    const g = stack.at(-1);
    if (g && !g.expandedStates && !g.break) g.break = "propagated";
  };
  prettier.doc.utils.traverseDoc(
    doc,
    (d) => {
      if (d.type === "break-parent") breakParent();
      if (d.type === "group") {
        stack.push(d);
        if (visited.has(d)) return false;
        visited.add(d);
      }
    },
    (d) => {
      if (d.type === "group" && stack.pop().break) breakParent();
    },
    true,
  );
}

export async function prettierDoc(stdin, opts) {
  const doc = await prettier.__debug.printToDoc(prepareText(stdin), opts);
  propagateBreaks(doc);
  return doc;
}

// The port's wire form -> JS objects shaped like Prettier's, with $def/$ref sharing restored.
export function fromWire(v) {
  const defs = new Map();
  const rec = (v) => {
    if (typeof v === "string") return v;
    if (Array.isArray(v)) return v.map(rec);
    if (v === null || typeof v !== "object") return v;
    if ("$utf16" in v) return String.fromCharCode(...v.$utf16);
    if ("$num" in v) return Number(v.$num === "-Infinity" ? -Infinity : v.$num === "Infinity" ? Infinity : NaN);
    if ("$ref" in v) return defs.get(v.$ref);
    if ("$def" in v) {
      // A shared subtree cannot contain itself, so building it before registering is safe.
      const d = rec(v.doc);
      defs.set(v.$def, d);
      return d;
    }
    const o = {};
    for (const [k, x] of Object.entries(v)) o[k] = k === "label" || k === "id" || k === "groupId" ? x : rec(x);
    return o;
  };
  return rec(v);
}

export function wireString(v) {
  return typeof v === "string" ? v : v && v.$utf16 ? String.fromCharCode(...v.$utf16) : v;
}

// ---------------------------------------------------------------------------------------------
// Canonicaliser. One `Canon` per case; both docs of the case go through the same instance so
// node numbers are comparable. Each canonical node / list is interned to a small integer
// (hash-consing), memoised per object, so shared subtrees cost once and equal docs get equal
// root numbers. `strict` turns off every normalisation except array flattening, shared-subtree
// expansion, group-id renumbering and `break` truthiness.
// ---------------------------------------------------------------------------------------------
const ALIGN_NORM = process.env.ALIGN_NORM === "1";
const isAscii = (s) => /^[\x00-\x7f]*$/.test(s);

export class Canon {
  constructor() {
    this.keys = new Map();
    this.reps = [];
  }
  intern(rep) {
    const k = JSON.stringify(rep);
    let n = this.keys.get(k);
    if (n === undefined) {
      n = this.reps.length;
      this.reps.push(rep);
      this.keys.set(k, n);
    }
    return n;
  }

  // Returns the list number of the root doc. Group ids are renumbered per doc.
  // `rebind`: number each referenced group definition separately and bind each reference to the
  // latest definition of that id before it (else the first after it), as if ids were unique
  // symbols. Diagnostic only: it hides id collisions, which the printer does see.
  root(doc, strict, rebind = false) {
    const refs = new Set();
    const seen = new Set();
    const walk = (d, f) => {
      // Visits every distinct object once, children in printing order.
      const stack = [d];
      while (stack.length) {
        const x = stack.pop();
        if (x === null || typeof x !== "object") continue;
        if (seen.has(x)) continue;
        seen.add(x);
        f(x);
        const kids = Array.isArray(x)
          ? x
          : x.type === "fill"
            ? x.parts
            : x.type === "if-break"
              ? [x.breakContents, x.flatContents]
              : x.type === "group"
                ? [x.contents, ...(x.expandedStates ?? [])]
                : [x.contents];
        for (let i = (kids?.length ?? 0) - 1; i >= 0; i--) stack.push(kids[i]);
      }
    };
    walk(doc, (x) => {
      if ((x.type === "if-break" || x.type === "indent-if-break") && x.groupId) refs.add(x.groupId);
    });
    const ids = new Map();
    seen.clear();
    walk(doc, (x) => {
      const id = x.type === "group" ? x.id : x.type === "if-break" || x.type === "indent-if-break" ? x.groupId : undefined;
      if (id && refs.has(id) && !ids.has(id)) ids.set(id, ids.size);
    });
    this.ids = ids;
    this.byObj = null;
    if (rebind) {
      const events = [];
      seen.clear();
      walk(doc, (x) => {
        if (x.type === "group" && x.id && refs.has(x.id)) events.push({ x, id: x.id, def: true });
        if ((x.type === "if-break" || x.type === "indent-if-break") && x.groupId) events.push({ x, id: x.groupId, def: false });
      });
      // A definition is identified by its contents (canonical form without ids), so a
      // group copied rather than shared (conditionalGroup states) stays one definition.
      this.strict = strict;
      this.memoItems = new Map();
      this.memoNode = new Map();
      this.noIds = true;
      this.list(doc);
      this.noIds = false;
      const normal = this.memoNode;
      const byObj = new Map();
      const keyNum = new Map();
      for (const e of events)
        if (e.def) {
          const k = normal.get(e.x);
          if (!keyNum.has(k)) keyNum.set(k, keyNum.size);
          byObj.set(e.x, keyNum.get(k));
        }
      const last = new Map();
      const pending = [];
      for (const e of events) {
        if (e.def) {
          last.set(e.id, byObj.get(e.x));
          for (let i = pending.length - 1; i >= 0; i--)
            if (pending[i].id === e.id) byObj.set(pending.splice(i, 1)[0].x, byObj.get(e.x));
        } else if (last.has(e.id)) byObj.set(e.x, last.get(e.id));
        else pending.push(e);
      }
      for (const e of pending) byObj.set(e.x, `unbound:${String(e.id.description ?? e.id)}`);
      this.byObj = byObj;
    }
    this.strict = strict;
    this.memoItems = new Map();
    this.memoNode = new Map();
    return this.list(doc);
  }

  gid(id, obj) {
    if (this.noIds) return null;
    if (this.byObj) return this.byObj.has(obj) ? this.byObj.get(obj) : null;
    return id && this.ids.has(id) ? this.ids.get(id) : null;
  }

  list(d) {
    return this.intern(["L", ...this.items(d)]);
  }

  // The flat item list of a doc in sequence position: strings and node numbers.
  items(d) {
    if (typeof d === "string") return d === "" && !this.strict ? [] : [d];
    if (d === undefined || d === null) return [];
    if (typeof d !== "object") return [this.intern(["invalid", typeof d])];
    const m = this.memoItems.get(d);
    if (m) return m;
    let out;
    if (Array.isArray(d)) {
      out = [];
      for (const c of d) for (const it of this.items(c)) this.push(out, it);
    } else if (!this.strict && d.type === "label") out = this.items(d.contents);
    else if (!this.strict && d.type === "break-parent") out = [];
    // align 0 / "" unwrapping (and negative n -> -1) is off: it hid the port printer handling
    // align("") differently from Prettier. ALIGN_NORM=1 restores it for comparison.
    else if (ALIGN_NORM && !this.strict && d.type === "align" && (d.n === 0 || d.n === "")) out = this.items(d.contents);
    else out = [this.node(d)];
    this.memoItems.set(d, out);
    return out;
  }

  push(out, it) {
    const last = out.at(-1);
    if (
      !this.strict &&
      typeof it === "string" &&
      typeof last === "string" &&
      ((isAscii(last) && isAscii(it)) || getStringWidth(last) + getStringWidth(it) === getStringWidth(last + it))
    )
      out[out.length - 1] = last + it;
    else out.push(it);
  }

  node(d) {
    const m = this.memoNode.get(d);
    if (m !== undefined) return m;
    const L = (x) => this.list(x);
    let rep;
    switch (d.type) {
      case "group":
        rep = ["group", d.break ? 1 : 0, this.gid(d.id, d), L(d.contents), d.expandedStates ? d.expandedStates.map(L) : null];
        break;
      case "fill":
        rep = ["fill", (d.parts ?? []).map(L)];
        break;
      case "if-break":
        rep = ["if-break", L(d.breakContents), L(d.flatContents), this.gid(d.groupId, d)];
        break;
      case "indent-if-break":
        rep = ["indent-if-break", L(d.contents), this.gid(d.groupId, d), d.negate ? 1 : 0];
        break;
      case "indent":
      case "line-suffix":
        rep = [d.type, L(d.contents)];
        break;
      case "align": {
        let n = d.n;
        if (typeof n === "number") n = n === -Infinity ? "-Infinity" : n < 0 && ALIGN_NORM && !this.strict ? -1 : n;
        else if (typeof n === "string") n = ["str", n];
        else if (n && n.type === "root") n = "root";
        else n = ["?", String(n)];
        rep = ["align", n, L(d.contents)];
        break;
      }
      case "line":
        rep = ["line", d.hard ? 1 : 0, d.soft ? 1 : 0, d.literal ? 1 : 0];
        break;
      case "label":
        rep = ["label", String(d.label), L(d.contents)];
        break;
      case "trim":
      case "cursor":
      case "line-suffix-boundary":
      case "break-parent":
        rep = [d.type];
        break;
      default:
        rep = ["invalid", String(d.type), d.message ?? null];
    }
    const n = this.intern(rep);
    this.memoNode.set(d, n);
    return n;
  }

  // Expands a list number back to readable JSON (for reports), up to `budget` nodes.
  expand(listId, budget = { n: 20000 }) {
    const node = (n) => {
      if (--budget.n < 0) return "…";
      const r = this.reps[n];
      const L = (x) => this.expand(x, budget);
      switch (r[0]) {
        case "group": {
          const o = { type: "group", break: !!r[1] };
          if (r[2] !== null) o.id = `#${r[2]}`;
          o.contents = L(r[3]);
          if (r[4]) o.expandedStates = r[4].map(L);
          return o;
        }
        case "fill": return { type: "fill", parts: r[1].map(L) };
        case "if-break": return { type: "if-break", ...(r[3] !== null ? { groupId: `#${r[3]}` } : {}), breakContents: L(r[1]), flatContents: L(r[2]) };
        case "indent-if-break": return { type: "indent-if-break", ...(r[2] !== null ? { groupId: `#${r[2]}` } : {}), negate: !!r[3], contents: L(r[1]) };
        case "indent": case "line-suffix": return { type: r[0], contents: L(r[1]) };
        case "align": return { type: "align", n: r[1], contents: L(r[2]) };
        case "line": return r[1] ? (r[3] ? "<literalline>" : "<hardline>") : r[2] ? "<softline>" : "<line>";
        case "label": return { type: "label", label: r[1], contents: L(r[2]) };
        default: return { type: r[0], ...(r.length > 1 ? { info: r.slice(1) } : {}) };
      }
    };
    const items = this.reps[listId].slice(1).map((it) => (typeof it === "string" ? it : node(it)));
    return items.length === 1 ? items[0] : items;
  }
}

// First place two expanded canonical docs differ.
export function firstDiff(a, b, p = "$") {
  if (typeof a !== typeof b || Array.isArray(a) !== Array.isArray(b)) return { path: p, a, b };
  if (typeof a !== "object" || a === null) return a === b ? null : { path: p, a, b };
  if (Array.isArray(a)) {
    for (let i = 0; i < Math.max(a.length, b.length); i++) {
      if (i >= a.length || i >= b.length) return { path: `${p}[${i}]`, a: a[i], b: b[i] };
      const d = firstDiff(a[i], b[i], `${p}[${i}]`);
      if (d) return d;
    }
    return null;
  }
  for (const k of new Set([...Object.keys(a), ...Object.keys(b)])) {
    const d = firstDiff(a[k], b[k], `${p}.${k}`);
    if (d) return d;
  }
  return null;
}

export function short(v, n = 300) {
  const s = JSON.stringify(v);
  return s === undefined ? "undefined" : s.length > n ? s.slice(0, n) + "…" : s;
}

// The port, as a long-lived child process answering one JSON line per request.
export class Port {
  constructor() {
    this.bin = path.join(TARGET_DIR, "release/doc-diff-rs");
    this.start();
  }
  start() {
    this.child = spawn(this.bin, [], { stdio: ["pipe", "pipe", "inherit"] });
    this.queue = [];
    // Split on "\n" only: readline also splits on U+2028/U+2029, which JSON leaves unescaped.
    let buf = [];
    this.child.stdout.on("data", (chunk) => {
      let start = 0;
      for (let i = chunk.indexOf(10); i !== -1; i = chunk.indexOf(10, start)) {
        buf.push(chunk.subarray(start, i));
        start = i + 1;
        const line = Buffer.concat(buf).toString("utf8");
        buf = [];
        const q = this.queue.shift();
        if (q) {
          clearTimeout(q.timer);
          q.resolve(JSON.parse(line));
        }
      }
      if (start < chunk.length) buf.push(chunk.subarray(start));
    });
    const child = this.child;
    child.on("exit", () => {
      if (this.child !== child) return;
      for (const q of this.queue.splice(0)) {
        clearTimeout(q.timer);
        q.resolve({ status: "crash" });
      }
      this.start();
    });
    child.stdin.on("error", () => {});
  }
  ask(req, timeoutMs = 120000) {
    return new Promise((resolve) => {
      const q = { resolve };
      q.timer = setTimeout(() => {
        this.child.kill("SIGKILL");
      }, timeoutMs);
      this.queue.push(q);
      this.child.stdin.write(JSON.stringify(req) + "\n");
    });
  }
  close() {
    this.child.removeAllListeners("exit");
    this.child.stdin.end();
  }
}
