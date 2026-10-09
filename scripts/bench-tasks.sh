#!/usr/bin/env bash
#
# Benchmarks multi-workspace task scheduling: generates a monorepo of N
# workspaces (random DAG, each depending on up to 3 earlier ones) with a
# trivial `build` script, then compares:
#
#   - yarn workspaces foreach -A --topological -p run build
#   - yarn tasks run --standalone -A build         (script-backed root default)
#   - yarn tasks run --standalone -A build         (inline `true` body)
#   - yarn tasks run --standalone -A build         (no script: pure scheduling)
#
# Usage: scripts/bench-tasks.sh [N=200] [RUNS=3]

set -euo pipefail

N=${1:-200}
RUNS=${2:-3}

HERE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
YARN_BIN=${YARN_BIN:-"$HERE_DIR/../target/release/yarn-bin"}

export YARN_ENABLE_TELEMETRY=0

BENCH_DIR=$(mktemp -d)
trap 'rm -rf "$BENCH_DIR"' EXIT

python3 - "$BENCH_DIR" "$N" <<'PY'
import json, os, random, sys

root, n = sys.argv[1], int(sys.argv[2])
random.seed(42)

json.dump({"name": "bench-root", "private": True, "workspaces": ["packages/*"]}, open(f"{root}/package.json", "w"))

for i in range(n):
    deps = {}
    if i > 0:
        for j in random.sample(range(i), min(i, random.randint(0, 3))):
            deps[f"pkg-{j:04d}"] = "workspace:*"

    folder = f"{root}/packages/pkg-{i:04d}"
    os.makedirs(folder)
    json.dump({"name": f"pkg-{i:04d}", "version": "1.0.0", "scripts": {"build": "true"}, "dependencies": deps}, open(f"{folder}/package.json", "w"))
PY

cd "$BENCH_DIR"
git init -q .
"$YARN_BIN" install > /dev/null

now() {
  python3 -c 'import time; print(time.time())'
}

measure() {
  local label=$1; shift

  for _ in $(seq "$RUNS"); do
    local start; start=$(now)
    "$@" > /dev/null 2>&1
    local end; end=$(now)
    python3 -c "print(f'{\"$label\":<40} {$end - $start:.3f}s ({($end - $start) * 1000 / $N:.2f}ms/workspace)')"
  done
}

printf '@workspaces\nbuild: ^build\n' > taskfile

measure "foreach --topological -p" "$YARN_BIN" workspaces foreach -A --topological -p run build
measure "tasks run (script-backed)" "$YARN_BIN" tasks run --standalone -A build

printf '@workspaces\nbuild: ^build\n  true\n' > taskfile
measure "tasks run (inline body)" "$YARN_BIN" tasks run --standalone -A build

printf '@workspaces\nbuild: ^build\n' > taskfile
for manifest in packages/*/package.json; do
  python3 - "$manifest" <<'PY'
import json, sys
manifest = json.load(open(sys.argv[1]))
manifest["scripts"] = {}
json.dump(manifest, open(sys.argv[1], "w"))
PY
done
measure "tasks run (no-op, scheduling only)" "$YARN_BIN" tasks run --standalone -A build
