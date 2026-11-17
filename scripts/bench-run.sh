#!/usr/bin/env bash
#
# Usage: bench-run.sh <package-manager> <fixture> <hyperfine|flamegraph> <bench-dir> [scenario]
#
# Package managers: zpm, zpm-nm, zpm-pnpm (Yarn 6 with the pnp, node-modules
# and pnpm linkers), yarn, yarn-nm, yarn-pnpm (Yarn 4.x, same linkers), npm,
# pnpm, classic (Yarn 1.x). Each one runs the binary found on the PATH, except
# Yarn 6 which runs `$ZPM_PATH` (default: `<repo>/local/yarn-bin`).
#
# Environment:
#   HYPERFINE_OPTIONS  extra hyperfine flags, word-split (e.g. `--runs 3`);
#                      `--runs`/`--min-runs` replace the default `--min-runs`
#   BENCH_MIN_RUNS     default minimum number of timed runs (10)
#   BENCH_NPM_HEAP_MB  heap size given to npm on the monorepo fixture (6144)
#   ZPM_PATH           Yarn 6 binary to benchmark
#
# The script exits with a non-zero status whenever an install fails, so that
# a broken package manager never produces data.

set -euo pipefail

PACKAGE_MANAGER=$1; shift
TEST_NAME=$1; shift
TEST_MODE=$1; shift
BENCH_DIR=$1; shift
SELECTED_SUBTEST=${1:-}

case "$SELECTED_SUBTEST" in
  ""|install-full-cold|install-cache-only|install-cache-and-lock|install-ready) ;;
  *)
    echo "Invalid benchmark scenario ${SELECTED_SUBTEST}"
    exit 1;;
esac

case "$TEST_MODE" in
  hyperfine|flamegraph) ;;
  *)
    echo "Invalid test mode ${TEST_MODE}"
    exit 1;;
esac

HERE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"

if [[ "$TEST_NAME" != "monorepo" && ! -f "$HERE_DIR/benchmarks/$TEST_NAME.json" ]]; then
  echo "Invalid fixture ${TEST_NAME}"
  exit 1
fi

BENCH_DIR=$(cd "$BENCH_DIR" && pwd)
echo "BENCH_DIR: $BENCH_DIR"
cd "$BENCH_DIR"

ZPM_PATH=${ZPM_PATH:-"${HERE_DIR}/../local/yarn-bin"}
FLAMEGRAPH=${FLAMEGRAPH:-"$HOME/.cargo/bin/flamegraph"}

case $PACKAGE_MANAGER in
  zpm|zpm-*)
    if [[ ! -x "$ZPM_PATH" ]]; then
      echo "Yarn 6 binary not found at $ZPM_PATH (build it or set ZPM_PATH)"
      exit 1
    fi
    ZPM_PATH="$(cd "$(dirname "$ZPM_PATH")" && pwd)/$(basename "$ZPM_PATH")";;
esac

# A RUST_BACKTRACE / RUST_LIB_BACKTRACE in the environment makes Yarn 6 capture
# a backtrace for every error it creates, including the expected ones it
# recovers from while linking (the node-modules and pnpm linkers become 10-30x
# slower). Users don't run with it, so the benchmarks don't either.
unset RUST_BACKTRACE RUST_LIB_BACKTRACE

# Every package manager gets a private home directory inside the benchmark
# directory, so that its caches and stores (~/.npm, ~/.cache/yarn, the pnpm
# store, Yarn's global folder, ...) start empty, never leak between runs, and
# get snapshotted / restored along with the project between iterations. The
# full cold scenario simply deletes it. Yarn Switch (which provides `yarn` on
# CI) keeps using its regular cache, since downloading the Yarn release itself
# isn't part of what we measure.
export YARNSW_CACHE_PATH=${YARNSW_CACHE_PATH:-"$HOME/.yarn/switch/cache"}

BENCH_HOME="$BENCH_DIR/.bench-home"
export HOME="$BENCH_HOME"
export XDG_CACHE_HOME="$BENCH_HOME/.cache"
export XDG_CONFIG_HOME="$BENCH_HOME/.config"
export XDG_DATA_HOME="$BENCH_HOME/.local/share"
export XDG_STATE_HOME="$BENCH_HOME/.local/state"
unset NPM_CONFIG_USERCONFIG npm_config_userconfig NPM_CONFIG_CACHE npm_config_cache

mkdir -p "$XDG_CACHE_HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_STATE_HOME"
CLEAR_BENCH_HOME=$(printf 'rm -rf %q && mkdir -p %q %q %q %q' "$BENCH_HOME" "$XDG_CACHE_HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_STATE_HOME")

BASELINE_DIR=$(mktemp -d)
PREPARE_DIR=$(mktemp -d)
trap 'rm -rf "$BASELINE_DIR" "$PREPARE_DIR"' EXIT
# Bash doesn't run the EXIT trap when killed by a signal it doesn't handle;
# the baseline snapshot can weigh gigabytes, so don't leave it behind
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP

HYPERFINE_ARGS=()
read -r -a HYPERFINE_ARGS <<< "${HYPERFINE_OPTIONS:-}"
RUN_COUNT_FLAGS='[[:space:]](-r|--runs|-m|--min-runs)[[:space:]=]'
if [[ ! " ${HYPERFINE_ARGS[*]-} " =~ $RUN_COUNT_FLAGS ]]; then
  HYPERFINE_ARGS+=("--min-runs=${BENCH_MIN_RUNS:-10}")
fi

sync-dir() {
  rsync -a --delete --exclude '/bench-*' --exclude '/flamegraphs/' "$1"/ "$2"/
}

write-prepare-script() {
  local subtest_name=$1
  local prepare_command=$2
  local prepare_script="$PREPARE_DIR/prepare-$subtest_name.sh"

  {
    echo '#!/usr/bin/env bash'
    echo 'set -e'
    printf 'rsync -a --delete --exclude %q --exclude %q %q/ %q/\n' '/bench-*' '/flamegraphs/' "$BASELINE_DIR" "$BENCH_DIR"
    printf 'cd %q\n' "$BENCH_DIR"
    printf '%s\n' "$prepare_command"
  } > "$prepare_script"

  chmod +x "$prepare_script"
  echo "$prepare_script"
}

bench() {
  SUBTEST_NAME=$1; shift
  PREPARE_COMMAND=$1; shift
  BENCH_COMMAND=$1; shift
  FLAMEGRAPH_COMMAND=${1:-}

  if [[ -n "$SELECTED_SUBTEST" && "$SUBTEST_NAME" != "$SELECTED_SUBTEST" ]]; then
    return
  fi

  echo "Testing $SUBTEST_NAME"
  echo "Preparing baseline state"

  # Every scenario starts from a successful install; benchmarking anything
  # else would only measure how fast the package manager fails
  local baseline_log="$PREPARE_DIR/baseline-$SUBTEST_NAME.log"
  if ! bash -c "$BASELINE_COMMAND" > "$baseline_log" 2>&1; then
    echo "::error::The baseline install ($BASELINE_COMMAND) failed:"
    tail -n 50 "$baseline_log"
    exit 1
  fi

  sync-dir "$BENCH_DIR" "$BASELINE_DIR"

  PREPARE_SCRIPT=$(write-prepare-script "$SUBTEST_NAME" "$PREPARE_COMMAND")

  if [[ "$TEST_MODE" == "flamegraph" ]]; then
    mkdir -p "$BENCH_DIR/flamegraphs"

    ("$PREPARE_SCRIPT" && bash -c "$BENCH_COMMAND") >& /dev/null || true

    "$PREPARE_SCRIPT"
    bash -c "$FLAMEGRAPH_COMMAND"
  fi

  if [[ "$TEST_MODE" == "hyperfine" ]]; then
    hyperfine "${HYPERFINE_ARGS[@]}" --export-json="bench-$SUBTEST_NAME.json" --warmup=1 --prepare="$PREPARE_SCRIPT" "$BENCH_COMMAND"
  fi
}

if [[ "$TEST_NAME" == "monorepo" ]]; then
  tar xfz "$HERE_DIR"/benchmarks/"$TEST_NAME".tgz
else
  cp "$HERE_DIR"/benchmarks/"$TEST_NAME".json package.json
fi

mkdir dummy-pkg
echo '{"name": "dummy-pkg", "version": "0.0.0"}' > dummy-pkg/package.json

touch a
if [[ "$(uname)" == "Darwin" ]]; then
  REFLINK_FLAG=-c
else
  REFLINK_FLAG=--reflink
fi
if cp "$REFLINK_FLAG" a b >& /dev/null; then
  echo "Reflinks are supported"
else
  echo "Reflink aren't supported! Installs may be quite slower than necessary"
fi
rm -f a b

# Dependency build scripts (postinstall & co) are disabled for every package
# manager: Yarn 6 doesn't run them by default, pnpm refuses to install
# dependencies with unapproved build scripts, and running them would mostly
# measure node-gyp and the packages' own scripts rather than the package
# manager. Each setup function below turns them off explicitly.
#
# The `install-ready` scenario adds a local folder dependency, which every
# package manager symlinks: `link:` for the Yarns and pnpm, `file:` for npm
# (which doesn't support `link:` but symlinks `file:` folders).
#
# The monorepo fixture is a Yarn project: it depends on its workspaces through
# the `workspace:` protocol, none of its workspaces have a version, and it
# uses `name@range` keys in `resolutions`. Yarn 4 and Yarn 6 install it as-is;
# bench-adapt-monorepo.js translates it for the other package managers.

adapt-monorepo() {
  node "$HERE_DIR/bench-adapt-monorepo.js" "$@"
}

setup-zpm() {
  export YARN_GLOBAL_FOLDER="${BENCH_HOME}/.yarn/berry"

  {
    echo "enableImmutableInstalls: false"
    echo "enableScripts: false"
    if [[ -n "${1:-}" ]]; then
      echo "nodeLinker: $1"
    fi
  } >> "$BENCH_DIR/.yarnrc.yml"
}

setup-yarn2() {
  yarn set version berry --yarn-path

  OLD_PATH=$(yarn config get yarnPath)
  NEW_PATH="${BENCH_DIR}/yarn.cjs"

  cp "$OLD_PATH" "$NEW_PATH"

  yarn config set yarnPath "$NEW_PATH"

  {
    echo "globalFolder: '${BENCH_HOME}/.yarn/berry'"
    echo "enableImmutableInstalls: false"
    echo "enableScripts: false"
  } >> "$BENCH_DIR/.yarnrc.yml"
}

setup-yarn2-nm() {
  {
    echo "nodeLinker: node-modules"
    echo "nmMode: hardlinks-local"
    echo "enableGlobalCache: true"
    echo "compressionLevel: 0"
  } >> "$BENCH_DIR/.yarnrc.yml"
}

setup-yarn2-pnpm() {
  {
    echo "nodeLinker: pnpm"
    echo "enableGlobalCache: true"
    echo "compressionLevel: 0"
  } >> "$BENCH_DIR/.yarnrc.yml"
}

setup-classic() {
  >> "$BENCH_DIR/.yarnrc" echo \
    "ignore-scripts true"

  if [[ "$TEST_NAME" == "monorepo" ]]; then
    adapt-monorepo classic
    # Yarn Classic refuses to add dependencies to the root of a workspace
    # project unless explicitly told to
    CLASSIC_ADD_FLAGS="--ignore-workspace-root-check"
  fi
}

setup-npm() {
  >> "$BENCH_DIR/.npmrc" echo \
    "ignore-scripts=true"

  if [[ "$TEST_NAME" == "monorepo" ]]; then
    adapt-monorepo npm
    # npm runs out of memory on the fixture with the default heap size
    export NODE_OPTIONS="--max-old-space-size=${BENCH_NPM_HEAP_MB:-6144} ${NODE_OPTIONS:-}"
  fi
}

setup-pnpm() {
  local pnpm_major
  # Outside of the project, since pnpm refuses to run in the monorepo fixture
  # (which declares another packageManager) until it's adapted below
  pnpm_major=$(cd "$PREPARE_DIR" && pnpm --version | cut -d. -f1)

  if [[ "$pnpm_major" -ge 11 ]]; then
    # pnpm 11+ only reads its settings from pnpm-workspace.yaml (camelCase)
    {
      echo "ignoreScripts: true"
      echo "strictPeerDependencies: false"
    } >> "$BENCH_DIR/pnpm-workspace.yaml"
  else
    {
      echo "ignore-scripts=true"
      echo "strict-peer-dependencies=false"
    } >> "$BENCH_DIR/.npmrc"
  fi

  if [[ "$TEST_NAME" == "monorepo" ]]; then
    adapt-monorepo pnpm "$pnpm_major"
    PNPM_ADD_FLAGS="--workspace-root"
  fi
}

# Records the exact version being measured (printed in the job summary), so
# that a change in the numbers can be matched with a release
record-version() {
  local version
  version=$(bash -c "$1" | tail -n 1)
  echo "Benchmarking $PACKAGE_MANAGER $version"
  echo "$version" > "$BENCH_DIR/bench-version.txt"
}

PNPM_ADD_FLAGS=""
CLASSIC_ADD_FLAGS=""

case $PACKAGE_MANAGER in
  zpm|zpm-nm|zpm-pnpm)
    case $PACKAGE_MANAGER in
      zpm) setup-zpm;;
      zpm-nm) setup-zpm node-modules;;
      zpm-pnpm) setup-zpm pnpm;;
    esac
    record-version "$ZPM_PATH --version"
    FLAMEGRAPH_PREFIX="$FLAMEGRAPH -o flamegraphs/$PACKAGE_MANAGER"
    BASELINE_COMMAND="$ZPM_PATH install"
    bench install-full-cold \
      "rm -rf .yarn .pnp.* node_modules yarn.lock && $CLEAR_BENCH_HOME" \
      "$ZPM_PATH install" \
      "$FLAMEGRAPH_PREFIX-install-full-cold.svg -- $ZPM_PATH install"
    bench install-cache-only \
      'rm -rf .yarn .pnp.* node_modules yarn.lock' \
      "$ZPM_PATH install" \
      "$FLAMEGRAPH_PREFIX-install-cache-only.svg -- $ZPM_PATH install"
    bench install-cache-and-lock \
      'rm -rf .yarn .pnp.* node_modules' \
      "$ZPM_PATH install" \
      "$FLAMEGRAPH_PREFIX-install-cache-and-lock.svg -- $ZPM_PATH install"
    bench install-ready \
      "$ZPM_PATH remove dummy-pkg || true" \
      "$ZPM_PATH add dummy-pkg@link:./dummy-pkg" \
      "$FLAMEGRAPH_PREFIX-install-ready.svg -- $ZPM_PATH add dummy-pkg@link:./dummy-pkg"
    ;;
  classic)
    setup-classic
    record-version 'yarn --version'
    BASELINE_COMMAND='yarn install'
    bench install-full-cold \
      "rm -rf node_modules yarn.lock && $CLEAR_BENCH_HOME" \
      'yarn install'
    bench install-cache-only \
      'rm -rf node_modules yarn.lock' \
      'yarn install'
    bench install-cache-and-lock \
      'rm -rf node_modules' \
      'yarn install'
    bench install-ready \
      "yarn remove $CLASSIC_ADD_FLAGS dummy-pkg || true" \
      "yarn add $CLASSIC_ADD_FLAGS dummy-pkg@link:./dummy-pkg"
    ;;
  yarn|yarn-nm|yarn-pnpm)
    setup-yarn2
    case $PACKAGE_MANAGER in
      yarn-nm) setup-yarn2-nm;;
      yarn-pnpm) setup-yarn2-pnpm;;
    esac
    record-version 'yarn --version'
    BASELINE_COMMAND='yarn install'
    bench install-full-cold \
      "rm -rf .yarn .pnp.* node_modules yarn.lock && $CLEAR_BENCH_HOME" \
      'yarn install'
    bench install-cache-only \
      'rm -rf .yarn .pnp.* node_modules yarn.lock' \
      'yarn install'
    bench install-cache-and-lock \
      'rm -rf .yarn .pnp.* node_modules' \
      'yarn install'
    bench install-ready \
      'yarn remove dummy-pkg || true' \
      'yarn add dummy-pkg@link:./dummy-pkg'
    ;;
  npm)
    setup-npm
    record-version 'npm --version'
    BASELINE_COMMAND='npm install'
    bench install-full-cold \
      "rm -rf node_modules package-lock.json && $CLEAR_BENCH_HOME" \
      'npm install'
    bench install-cache-only \
      'rm -rf node_modules package-lock.json' \
      'npm install'
    bench install-cache-and-lock \
      'rm -rf node_modules' \
      'npm install'
    bench install-ready \
      'npm remove dummy-pkg || true' \
      'npm add dummy-pkg@file:./dummy-pkg'
    ;;
  pnpm)
    setup-pnpm
    record-version 'pnpm --version'
    BASELINE_COMMAND='pnpm install'
    bench install-full-cold \
      "rm -rf node_modules pnpm-lock.yaml && $CLEAR_BENCH_HOME" \
      'pnpm install'
    bench install-cache-only \
      'rm -rf node_modules pnpm-lock.yaml' \
      'pnpm install'
    bench install-cache-and-lock \
      'rm -rf node_modules' \
      'pnpm install'
    bench install-ready \
      "pnpm remove $PNPM_ADD_FLAGS dummy-pkg || true" \
      "pnpm add $PNPM_ADD_FLAGS dummy-pkg@link:./dummy-pkg"
    ;;
  *)
    echo "Invalid package manager ${PACKAGE_MANAGER}"
    exit 1;;
esac
