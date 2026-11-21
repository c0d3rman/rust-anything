#!/bin/sh
# scripts/setup.sh <work dir>
# Clones ESLint v10.15.0 and the three real repositories, installs ESLint's production
# dependencies, and builds the app folder the cases are recorded in:
#   <work>/eslint         the clone (with wrappers/eslint-stdin.js)
#   <work>/eslint-app     node_modules/eslint (the original) and rust/app -> this folder's app
#   <work>/real/{ms,Fuse,json-server}
#   <work>/deps           `globals`, which ESLint's rule tests import
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
work=$1
mkdir -p "$work" && cd "$work" && work=$(pwd)

git clone -q --depth 1 --branch v10.15.0 https://github.com/eslint/eslint.git eslint
test "$(git -C eslint rev-parse HEAD)" = a2101d992ab8a5d49ab7c960e9f5f67707530703
(cd eslint && npm install --omit=dev --ignore-scripts --no-audit --no-fund)
mkdir -p eslint/wrappers && cp "$here/wrappers/eslint-stdin.js" eslint/wrappers/

mkdir -p deps && (cd deps && npm init -y >/dev/null && npm install --ignore-scripts --no-audit --no-fund globals@16)

mkdir -p real
git clone -q --depth 1 --branch 3.0.0-canary.1 https://github.com/vercel/ms.git real/ms
git clone -q --depth 1 --branch v7.1.0 https://github.com/krisk/Fuse.git real/Fuse
git clone -q --depth 1 --branch v1.0.0-beta.3 https://github.com/typicode/json-server.git real/json-server

mkdir -p eslint-app/node_modules eslint-app/rust
rsync -a --exclude tests --exclude docs --exclude .git --exclude packages/js/node_modules eslint/ eslint-app/node_modules/eslint/
ln -s "$here/app" eslint-app/rust/app

node "$here/scripts/harvest-rule-tests.cjs" "$work/eslint" "$here/scripts/rules.txt" rule-inputs.jsonl "$work/deps"
node "$here/scripts/gen-real-inputs.cjs" "$here/scripts/rules.txt" real-inputs.jsonl \
	ms=real/ms Fuse=real/Fuse json-server=real/json-server eslint-lib=eslint/lib eslint-bin=eslint/bin
node "$here/scripts/gen-core-inputs.cjs" core-inputs.jsonl
cat rule-inputs.jsonl real-inputs.jsonl core-inputs.jsonl > inputs.jsonl
wc -l inputs.jsonl
