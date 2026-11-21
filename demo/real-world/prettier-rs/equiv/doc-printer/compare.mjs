// Prints docs with both Prettier and the Rust port and compares the results.
import { rust, printDocToString } from "./lib.mjs";
import { materialize } from "./grammar.mjs";

export const WIDTHS = [0, 1, 2, 3, 4, 5, 6, 8, 12, 80, Infinity];
const TABS = [[false, 2], [false, 4], [true, 2], [true, 4]];
const mk = (printWidth, useTabs, tabWidth, endOfLine = "lf") => ({ printWidth, tabWidth, useTabs, endOfLine });
// useTabs/tabWidth are read only when building an indentation (`makeIndent`/`makeAlign` →
// `generateInd` in printer.js; `generate_ind` in port.rs), so docs with no indent / align /
// indent-if-break get one tab setting only.
export const OPTS_INDENT = [...WIDTHS.flatMap(w => TABS.map(([t, n]) => mk(w, t, n))), mk(4, false, 2, "crlf"), mk(4, true, 4, "cr")];
export const OPTS_PLAIN = [...WIDTHS.map(w => mk(w, false, 2)), mk(4, false, 2, "crlf")];
const wire = o => o.map(x => x.printWidth === Infinity ? { ...x, printWidth: { $num: "Infinity" } } : x);
const OPTS_INDENT_J = JSON.stringify(wire(OPTS_INDENT));
const OPTS_PLAIN_J = JSON.stringify(wire(OPTS_PLAIN));

export function jsPrint(docString, opts) {
  const doc = materialize(docString);
  return opts.map(o => {
    try {
      const r = printDocToString(doc, o);
      return ["ok", r.formatted, r.cursorNodeStart ?? null, r.cursorNodeText ?? null];
    } catch (e) {
      return ["err", String(e?.message ?? e)];
    }
  });
}

export class Comparer {
  constructor(onMismatch) {
    this.onMismatch = onMismatch;
    this.docs = 0; this.prints = 0; this.mismatches = 0; this.errBoth = 0; this.errMsgDiff = 0;
    this.batches = { indent: [], plain: [] };
  }
  add(docString, indented) {
    const b = this.batches[indented ? "indent" : "plain"];
    b.push(docString);
    if (b.length >= 2000) this.flush(indented);
  }
  flushAll() { this.flush(true); this.flush(false); }
  flush(indented) {
    const key = indented ? "indent" : "plain";
    const docs = this.batches[key];
    if (!docs.length) return;
    this.batches[key] = [];
    const opts = indented ? OPTS_INDENT : OPTS_PLAIN;
    const rs = JSON.parse(rust.printBatch(`[${docs.join(",")}]`, indented ? OPTS_INDENT_J : OPTS_PLAIN_J));
    docs.forEach((d, i) => {
      const js = jsPrint(d, opts);
      this.docs++;
      this.prints += opts.length;
      js.forEach((j, k) => {
        const r = rs[i][k];
        if (j[0] === "err" && r[0] === "err") {
          this.errBoth++;
          if (j[1] !== r[1]) { this.errMsgDiff++; this.onMismatch("errmsg", d, opts[k], j, r); }
          return;
        }
        if (j.length !== r.length || j.some((x, m) => x !== r[m])) {
          this.mismatches++;
          this.onMismatch("output", d, opts[k], j, r);
        }
      });
    });
  }
}
