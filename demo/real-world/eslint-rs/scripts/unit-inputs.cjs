"use strict";

// node unit-inputs.cjs <inputs.jsonl> <claimed names.txt> <out.jsonl>
// The `lintStdin(args, text)` calls for the CLI cases the port claims (case names as the
// recorder slugs them: lower case).

const fs = require("node:fs");

const [inputs, claimedFile, out] = process.argv.slice(2);
const claimed = new Set(fs.readFileSync(claimedFile, "utf8").split("\n").filter(Boolean));
const lines = [];
for (const line of fs.readFileSync(inputs, "utf8").split("\n")) {
	if (!line) {
		continue;
	}
	const c = JSON.parse(line);
	if (claimed.has(c.name.toLowerCase())) {
		lines.push(JSON.stringify({ fn: "lintStdin", args: [c.args, c.stdin ?? ""] }));
	}
}
fs.writeFileSync(out, lines.length ? `${lines.join("\n")}\n` : "");
console.error(`${lines.length} calls`);
