#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: yarn run e2e <test-name>" >&2
  echo "Example: yarn run e2e smoke" >&2
  exit 1
fi

TEST_NAME="${1%.sh}"
TEST_SCRIPT=$(realpath "tests/e2e/${TEST_NAME}.sh")

if [[ ! -f "${TEST_SCRIPT}" ]]; then
  echo "Unknown e2e test: ${TEST_NAME}" >&2
  echo "Available tests:" >&2
  find tests/e2e -maxdepth 1 -type f -name '*.sh' -exec basename {} .sh \; | sort >&2
  exit 1
fi

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
BIN_YARN_SWITCH="${REPO_ROOT}/target/release/yarn"
BIN_YARN="${REPO_ROOT}/target/release/yarn-bin"

if [[ ! -x "${BIN_YARN_SWITCH}" || ! -x "${BIN_YARN}" ]]; then
  echo "Missing compiled Yarn binary at ${BIN_YARN_SWITCH} or ${BIN_YARN}" >&2
  exit 1
fi

TEMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "${TEMP_DIR}"
}
trap cleanup EXIT

mkdir -p "${TEMP_DIR}/bin"
ln -sf "${BIN_YARN_SWITCH}" "${TEMP_DIR}/bin/yarn"

export PATH="${TEMP_DIR}/bin:${PATH}"
export YARNSW_DEFAULT="local:${BIN_YARN}"

PROJECT_DIR="${TEMP_DIR}/project"
mkdir -p "${PROJECT_DIR}"
cd "${PROJECT_DIR}"

function wait_for() {
  local url="${1}"
  for _ in {1..30}; do
    if curl -fsS "${url}" > /dev/null; then
      break
    fi
    sleep 1
  done
}

set -x

source "${TEST_SCRIPT}"
