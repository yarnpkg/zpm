#!/usr/bin/env bash
# Benchmarks the task cache on a generated monorepo.
#
#   scripts/benchmarks/task-cache.sh [workspace-count] [fanout]
#
# Generates N workspaces (default 200) where pkg-i depends on the `fanout`
# previous ones (default 3), each with a cached `build` task (trivial work:
# copy its sources and dependency outputs into dist/), plus a root `build`
# aggregating everything. Measures:
#   - cold run (empty cache)
#   - warm run, fully cached, outputs already on disk
#   - warm run after touching one leaf input (only that task misses)
#   - warm run after touching the root of the chain (everything misses)
#   - warm run after deleting all outputs (everything restored)
set -euo pipefail

COUNT=${1:-200}
FANOUT=${2:-3}
ROOT_DIR=$(cd "$(dirname "$0")/../.." && pwd)
YARN=${YARN_BIN:-$ROOT_DIR/target/release/yarn-bin}
BENCH_DIR=$(mktemp -d "${TMPDIR:-/tmp}/task-cache-bench.XXXXXX")

export YARN_IGNORE_PATH=1
export YARN_ENABLE_TELEMETRY=0
export YARN_ENABLE_PROGRESS_BARS=0

cd "$BENCH_DIR"
git init -q
echo "dist" > .gitignore

DEPS_JSON=""
for i in $(seq 1 "$COUNT"); do
  mkdir -p "packages/pkg-$i/src"
  echo "export const value$i = $i;" > "packages/pkg-$i/src/index.js"

  deps=()
  task_deps=()
  for ((d = (i > FANOUT ? i - FANOUT : 1); d < i; d++)); do
    deps+=("\"pkg-$d\": \"workspace:*\"")
    task_deps+=("pkg-$d:build&")
  done

  (IFS=,; echo "{\"name\": \"pkg-$i\", \"dependencies\": {${deps[*]:-}}}") > "packages/pkg-$i/package.json"

  cat > "packages/pkg-$i/taskfile" <<TASKFILE
@cache
@inputs(src/**)
@outputs(dist/**)
build: ${task_deps[*]:-}
  mkdir -p dist && cp src/index.js dist/index.js
TASKFILE

  DEPS_JSON="$DEPS_JSON\"pkg-$i\": \"workspace:*\","
done

echo "{\"name\": \"bench-root\", \"private\": true, \"workspaces\": [\"packages/*\"], \"dependencies\": {${DEPS_JSON%,}}}" > package.json

ROOT_DEPS=""
for i in $(seq 1 "$COUNT"); do
  ROOT_DEPS="$ROOT_DEPS pkg-$i:build&"
done
printf 'build:%s\n' "$ROOT_DEPS" > taskfile

"$YARN" install > /dev/null

run() {
  local label=$1
  shift
  local start end
  start=$(perl -MTime::HiRes=time -e 'printf "%.3f", time')
  "$YARN" tasks run --standalone "$@" build > /dev/null 2> "$BENCH_DIR/last-stderr.log"
  end=$(perl -MTime::HiRes=time -e 'printf "%.3f", time')
  local hits
  hits=$(grep -c "Cache hit" "$BENCH_DIR/last-stderr.log" || true)
  printf '%-48s %6.3fs  (%s cache hits)\n' "$label" "$(echo "$end - $start" | bc)" "$hits"
}

echo "Workspaces: $COUNT (fanout $FANOUT) in $BENCH_DIR"
run "cold (empty cache)"
run "warm, fully cached"
run "warm, fully cached (again)"
echo "// touched" >> "packages/pkg-$COUNT/src/index.js"
run "warm, one leaf input touched"
run "warm, fully cached after leaf rebuild"
echo "// touched" >> "packages/pkg-1/src/index.js"
run "warm, chain root input touched (all miss)"
rm -rf packages/*/dist
run "warm, all outputs deleted (all restored)"
run "--no-cache (all run)" --no-cache

if [ -z "${KEEP:-}" ]; then
  rm -rf "$BENCH_DIR"
fi
