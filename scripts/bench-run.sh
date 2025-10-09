#!/usr/bin/env bash

set -e

PACKAGE_MANAGER=$1; shift
TEST_NAME=$1; shift
TEST_MODE=$1; shift
BENCH_DIR=$1; shift

HERE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"

echo "BENCH_DIR: $BENCH_DIR"
cd "$BENCH_DIR"

bench() {
  SUBTEST_NAME=$1; shift
  PREPARE_COMMAND=$1; shift
  BENCH_COMMAND=$1; shift
  FLAMEGRAPH_COMMAND=$1; shift

  echo "Testing $SUBTEST_NAME"

  if [[ "$TEST_MODE" == "flamegraph" ]]; then
    mkdir -p "$BENCH_DIR/flamegraphs"

    (bash -c "$PREPARE_COMMAND" && bash -c "$BENCH_COMMAND") >& /dev/null || true

    bash -c "$PREPARE_COMMAND"
    bash -c "$FLAMEGRAPH_COMMAND"
  fi

  if [[ "$TEST_MODE" == "hyperfine" ]]; then
    hyperfine ${HYPERFINE_OPTIONS:-} --export-json=bench-$SUBTEST_NAME.json --min-runs=10 --warmup=1 --prepare="$PREPARE_COMMAND" "$BENCH_COMMAND"
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
if cp --reflink a b >& /dev/null; then
  echo "Reflinks are supported"
else
  echo "Reflink aren't supported! Installs may be quite slower than necessary"
fi

ls -l "${HERE_DIR}/../local/"
ZPM_PATH="${HERE_DIR}/../local/yarn-bin"

setup-zpm() {
  export YARN_GLOBAL_FOLDER="${BENCH_DIR}/.yarn-global"

  >> "$BENCH_DIR/.yarnrc.yml" echo \
    "enableImmutableInstalls: false"

  if [[ "$TEST_NAME" == "monorepo" ]]; then
    >> "$BENCH_DIR/.yarnrc.yml" echo \
      "enableScripts: false"
  fi
}

setup-yarn2() {
  yarn set version berry --yarn-path

  OLD_PATH=$(yarn config get yarnPath)
  NEW_PATH="${BENCH_DIR}/yarn.cjs"

  cp "$OLD_PATH" "$NEW_PATH"

  yarn config set yarnPath "$NEW_PATH"

  >> "$BENCH_DIR/.yarnrc.yml" echo \
    "globalFolder: '${BENCH_DIR}/.yarn-global'"
  >> "$BENCH_DIR/.yarnrc.yml" echo \
    "enableImmutableInstalls: false"
}

setup-yarn2-nm() {
  >> "$BENCH_DIR/.yarnrc.yml" echo \
    "nodeLinker: node-modules"
  >> "$BENCH_DIR/.yarnrc.yml" echo \
    "nmMode: hardlinks-local"
  >> "$BENCH_DIR/.yarnrc.yml" echo \
    "enableGlobalCache: true"
  >> "$BENCH_DIR/.yarnrc.yml" echo \
    "compressionLevel: 0"
}

setup-yarn2-pnpm() {
  >> "$BENCH_DIR/.yarnrc.yml" echo \
    "nodeLinker: pnpm"
  >> "$BENCH_DIR/.yarnrc.yml" echo \
    "enableGlobalCache: true"
  >> "$BENCH_DIR/.yarnrc.yml" echo \
    "compressionLevel: 0"
}

setup-pnpm() {
  >> "$BENCH_DIR/.npmrc" echo \
    "strict-peer-dependencies=false"
}

case $PACKAGE_MANAGER in
  zpm)
    setup-zpm
    bench install-full-cold \
      'rm -rf .yarn .pnp.* yarn.lock .yarn-global' \
      "$ZPM_PATH install" \
      "~/.cargo/bin/flamegraph -o flamegraphs/zpm-install-full-cold.svg -- $ZPM_PATH install"
    bench install-cache-only \
      'rm -rf .yarn .pnp.* yarn.lock' \
      "$ZPM_PATH install" \
      "~/.cargo/bin/flamegraph -o flamegraphs/zpm-install-cache-only.svg -- $ZPM_PATH install"
    bench install-cache-and-lock \
      'rm -rf .yarn .pnp.*' \
      "$ZPM_PATH install" \
      "~/.cargo/bin/flamegraph -o flamegraphs/zpm-install-cache-and-lock.svg -- $ZPM_PATH install"
    bench install-ready \
      "$ZPM_PATH remove dummy-pkg || true" \
      "$ZPM_PATH add dummy-pkg@link:./dummy-pkg" \
      "~/.cargo/bin/flamegraph -o flamegraphs/zpm-install-ready.svg -- $ZPM_PATH add dummy-pkg@link:./dummy-pkg"
    ;;
  classic)
    bench install-full-cold \
      'rm -rf node_modules yarn.lock && yarn cache clean' \
      'yarn install'
    bench install-cache-only \
      'rm -rf node_modules yarn.lock' \
      'yarn install'
    bench install-cache-and-lock \
      'rm -rf node_modules' \
      'yarn install'
    bench install-ready \
      'yarn remove dummy-pkg || true' \
      'yarn add dummy-pkg@link:./dummy-pkg'
    ;;
  yarn)
    setup-yarn2
    bench install-full-cold \
      'rm -rf .yarn .pnp.* yarn.lock .yarn-global' \
      'yarn install'
    bench install-cache-only \
      'rm -rf .yarn .pnp.* yarn.lock' \
      'yarn install'
    bench install-cache-and-lock \
      'rm -rf .yarn .pnp.*' \
      'yarn install'
    bench install-ready \
      'yarn remove dummy-pkg || true' \
      'yarn add dummy-pkg@link:./dummy-pkg'
    ;;
  yarn-nm)
    setup-yarn2
    setup-yarn2-nm
    bench install-full-cold \
      'rm -rf .yarn node_modules yarn.lock .yarn-global' \
      'yarn install'
    bench install-cache-only \
      'rm -rf .yarn node_modules yarn.lock' \
      'yarn install'
    bench install-cache-and-lock \
      'rm -rf .yarn node_modules' \
      'yarn install'
    bench install-ready \
      'yarn remove dummy-pkg || true' \
      'yarn add dummy-pkg@link:./dummy-pkg'
    ;;
  yarn-pnpm)
    setup-yarn2
    setup-yarn2-pnpm
    bench install-full-cold \
      'rm -rf .yarn node_modules yarn.lock .yarn-global' \
      'yarn install'
    bench install-cache-only \
      'rm -rf .yarn node_modules yarn.lock' \
      'yarn install'
    bench install-cache-and-lock \
      'rm -rf .yarn node_modules' \
      'yarn install'
    bench install-ready \
      'yarn remove dummy-pkg || true' \
      'yarn add dummy-pkg@link:./dummy-pkg'
    ;;
  npm)
    bench install-full-cold \
      'rm -rf node_modules package-lock.json && npm cache clean --force' \
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
    bench install-full-cold \
      'rm -rf node_modules pnpm-lock.yaml ~/.local/share/pnpm/store ~/.cache/pnpm' \
      'pnpm install'
    bench install-cache-only \
      'rm -rf node_modules pnpm-lock.yaml' \
      'pnpm install'
    bench install-cache-and-lock \
      'rm -rf node_modules' \
      'pnpm install'
    bench install-ready \
      'pnpm remove dummy-pkg || true' \
      'pnpm add dummy-pkg@link:./dummy-pkg'
    ;;
  *)
    echo "Invalid package manager ${PACKAGE_MANAGER}"
    exit 1;;
esac
