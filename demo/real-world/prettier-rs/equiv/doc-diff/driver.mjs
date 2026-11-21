// Doc-level differential check, one shard of the corpus.
//
//   node driver.mjs base  <inputs.jsonl> <out.jsonl> <shard>/<shards> [name-regex]
//   node driver.mjs sweep <inputs.jsonl> <out.jsonl> <shard>/<shards> [name-regex]
//
// base: for every case, the port's doc and Prettier's doc at the case's own options, compared
//   after canonicalisation, plus both final strings.
// sweep: for every case the port formats, both docs at a list of print widths (see widthsFor), and
//   for every case whose Prettier doc changes with the width, both final strings at every width
//   0..200.
// The work runs in a worker thread with a large stack (deep docs).
import { Worker, isMainThread, workerData } from "node:worker_threads";
import fs from "node:fs";

if (isMainThread) {
  const w = new Worker(new URL(import.meta.url), {
    workerData: process.argv.slice(2),
    resourceLimits: { stackSizeMb: 256, maxOldGenerationSizeMb: 8192 },
  });
  w.on("error", (e) => {
    console.error(e);
    process.exit(1);
  });
  w.on("exit", (c) => process.exit(c));
} else {
  const { prettier, flagsToOptions, language, prettierDoc, fromWire, wireString, Canon, firstDiff, short, Port } =
    await import("./lib.mjs");
  const [mode, inputs, outPath, shardSpec, only] = workerData;
  const [shard, shards] = shardSpec.split("/").map(Number);
  const onlyRe = only ? new RegExp(only) : null;
  const cases = fs
    .readFileSync(inputs, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((l) => JSON.parse(l))
    .filter((c, i) => i % shards === shard && (!onlyRe || onlyRe.test(c.name)));
  const port = new Port();
  const out = fs.openSync(outPath, "w");
  const emit = (r) => fs.writeSync(out, JSON.stringify(r) + "\n");

  const prettierFormat = async (stdin, opts) => {
    try {
      return { ok: await prettier.format(stdin, opts) };
    } catch (e) {
      return { err: String(e.message).split("\n")[0] };
    }
  };

  // Compares both docs of one case at one set of options. Returns the record fields.
  async function compareDocs(c, opts, width, canon) {
    const req = { args: c.args, stdin: c.stdin, doc: true, fmt: false };
    if (width !== undefined) req.width = width;
    const r = await port.ask(req);
    if (r.status !== "ok") return { port: r.status, err: r.err };
    let pdoc;
    try {
      pdoc = await prettierDoc(c.stdin, width === undefined ? opts : { ...opts, printWidth: width });
    } catch (e) {
      return { port: "ok", prettierErr: String(e.message).split("\n")[0] };
    }
    const rdoc = fromWire(r.doc);
    const p = canon.root(pdoc, false);
    const q = canon.root(rdoc, false);
    const res = { port: "ok", docEq: p === q, pRoot: p, rRoot: q };
    if (p !== q) {
      const a = canon.expand(p);
      const b = canon.expand(q);
      const d = firstDiff(a, b);
      res.diff = d ? { path: d.path, prettier: short(d.a), port: short(d.b) } : { path: "(beyond expansion budget)" };
    }
    res.strictEq = canon.root(pdoc, true) === canon.root(rdoc, true);
    if (p !== q) res.reboundEq = canon.root(pdoc, false, true) === canon.root(rdoc, false, true);
    return res;
  }

  const widthsFor = (lang, stdin) => {
    // JS/TS: the only printWidth read is isLoneShortArgument (`len <= printWidth * 0.25`, integer
    // len), so the doc is constant on [4k, 4k+3]; every 4th width covers 0..203, and 1e6 the
    // "everything is short" end. Markdown also prints table cells with printDocToString at the
    // real width, so a table means every width; a code fence embeds JS. Other languages read no
    // width: a spread of widths checks that empirically.
    const step4 = Array.from({ length: 51 }, (_, i) => i * 4);
    if (lang === "js" || lang === "ts") return [...step4, 1e6];
    // Markdown without a table or a code fence has no width read; it gets the spread below.
    if (lang === "markdown" && stdin.includes("|")) return [...Array(201).keys(), 1e6];
    if (lang === "markdown" && /```|~~~/.test(stdin)) return [...step4, 1e6];
    return [0, 1, 2, 3, 5, 10, 20, 40, 60, 79, 80, 81, 100, 120, 160, 200, 1e6];
  };

  let n = 0;
  for (const c of cases) {
    n++;
    const lang = language(c.args);
    let opts;
    try {
      opts = flagsToOptions(c.args);
    } catch (e) {
      emit({ name: c.name, lang, skip: String(e.message) });
      continue;
    }
    const canon = new Canon();
    try {
      if (mode === "base") {
        const r = await port.ask({ args: c.args, stdin: c.stdin, doc: false, fmt: true });
        const rec = { name: c.name, lang };
        if (!r.fmt || !("ok" in r.fmt)) {
          rec.port = r.status === "ok" ? "fmt-err" : r.status;
          rec.err = r.err ?? r.fmt?.err;
          emit(rec);
          continue;
        }
        const pf = await prettierFormat(c.stdin, opts);
        rec.strEq = "ok" in pf && pf.ok === wireString(r.fmt.ok);
        if (!("ok" in pf)) rec.prettierFmtErr = pf.err;
        if (r.status === "empty") {
          rec.port = "empty";
          emit(rec);
          continue;
        }
        Object.assign(rec, await compareDocs(c, opts, undefined, canon));
        delete rec.pRoot;
        delete rec.rRoot;
        emit(rec);
      } else {
        if (c.stdin.length > 60000) {
          emit({ name: c.name, lang, skip: "large" });
          continue;
        }
        // SWEEP_ALL=1: every width 0..200 for every case, and the widths where Prettier's doc
        // changes (`pChangesAt`), to check the step-4 argument in widthsFor.
        const widths = process.env.SWEEP_ALL ? [...Array(201).keys(), 1e6] : widthsFor(lang, c.stdin);
        let prevP;
        const rec = { name: c.name, lang, widths: widths.length, docMismatchWidths: [], pDistinct: 0, rDistinct: 0 };
        const pRoots = new Set();
        const rRoots = new Set();
        let skip = null;
        for (const w of widths) {
          const r = await compareDocs(c, opts, w, canon);
          if (r.port !== "ok" || r.prettierErr) {
            skip = r.port !== "ok" ? `port ${r.port} at ${w}` : `prettier err at ${w}: ${r.prettierErr}`;
            if (w === widths[0]) break;
            rec.partial = skip;
            continue;
          }
          if (prevP !== undefined && r.pRoot !== prevP) (rec.pChangesAt ??= []).push(w);
          prevP = r.pRoot;
          pRoots.add(r.pRoot);
          rRoots.add(r.rRoot);
          if (!r.docEq) {
            rec.docMismatchWidths.push(w);
            if (!r.reboundEq) (rec.unexplainedWidths ??= []).push(w);
            rec.diff ??= { width: w, reboundEq: r.reboundEq, ...r.diff };
          }
        }
        if (skip && pRoots.size === 0) {
          emit({ name: c.name, lang, skip });
          continue;
        }
        rec.pDistinct = pRoots.size;
        rec.rDistinct = rRoots.size;
        if (pRoots.size > 1 || rRoots.size > 1 || rec.docMismatchWidths.length) {
          rec.strMismatchWidths = [];
          for (let w = 0; w <= 200; w++) {
            const r = await port.ask({ args: c.args, stdin: c.stdin, width: w, doc: false, fmt: true });
            const pf = await prettierFormat(c.stdin, { ...opts, printWidth: w });
            const a = pf.ok;
            const b = r.fmt && "ok" in r.fmt ? wireString(r.fmt.ok) : undefined;
            if (a === undefined || b === undefined) {
              if (a !== b) rec.strMismatchWidths.push(`${w}:${a === undefined ? "prettier-err" : "port-err"}`);
            } else if (a !== b) rec.strMismatchWidths.push(w);
          }
        }
        emit(rec);
      }
    } catch (e) {
      emit({ name: c.name, lang, error: String(e?.stack ?? e).slice(0, 500) });
    }
    if (n % 500 === 0) console.error(`[${mode} ${shard}/${shards}] ${n}/${cases.length}`);
  }
  port.close();
  fs.closeSync(out);
  console.error(`[${mode} ${shard}/${shards}] done ${n}`);
}
