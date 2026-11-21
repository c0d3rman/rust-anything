// Shared loaders: Prettier 3.6.2 (the oracle) and the Rust port's napi harness.
import { createRequire } from "node:module";
import path from "node:path";

const here = path.dirname(new URL(import.meta.url).pathname);
// PRETTIER_DIR: a Prettier 3.6.2 package directory (the npm package, or a build's
// dist/prettier). Without it, `prettier` is resolved from node_modules as usual.
export const PRETTIER_DIR = process.env.PRETTIER_DIR;
export const prettier = await import(PRETTIER_DIR ? path.join(PRETTIER_DIR, "index.mjs") : "prettier").catch(e => {
  throw new Error(`Cannot load Prettier (${e.message}). Set PRETTIER_DIR or run \`npm install --prefix . --no-save prettier@3.6.2\` here.`);
});
if (prettier.version !== "3.6.2") console.warn(`warning: Prettier ${prettier.version}; the port follows 3.6.2`);
export const printDocToString = prettier.doc.printer.printDocToString;
export const getStringWidth = prettier.util.getStringWidth;
export const rust = createRequire(import.meta.url)(path.join(here, "harness/doc_printer_equiv.node"));
export { here };
