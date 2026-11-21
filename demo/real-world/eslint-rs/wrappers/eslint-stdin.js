"use strict";

const util = require("node:util");
const cli = require("../lib/cli");

/**
 * Runs ESLint's CLI on piped-in text, in process, and returns what it prints.
 * The working directory in the output is written as `[CWD]`.
 * @param {string[]} args The arguments after `eslint`.
 * @param {string} text The text on stdin.
 * @returns {Promise<string>} JSON of `{ stdout, stderr, code }`.
 */
async function lintStdin(args, text) {
	const out = [];
	const err = [];
	const { log, error, info, warn } = console;
	console.log = console.info = (...a) => out.push(`${util.format(...a)}\n`);
	console.error = console.warn = (...a) => err.push(`${util.format(...a)}\n`);
	let code;
	try {
		code = await cli.execute(["node", "eslint", ...args], text);
	} finally {
		Object.assign(console, { log, error, info, warn });
	}
	const cwd = process.cwd();
	const clean = s => s.split(cwd).join("[CWD]");
	return JSON.stringify({ stdout: clean(out.join("")), stderr: clean(err.join("")), code });
}

module.exports = { lintStdin };
