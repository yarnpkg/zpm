#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: yarn run e2e <test-name>" >&2
  echo "Example: yarn run e2e smoke" >&2
  exit 1
fi

TEST_NAME="${1%.sh}"
TEST_SCRIPT="tests/e2e/${TEST_NAME}.sh"

if [[ ! -f "${TEST_SCRIPT}" ]]; then
  echo "Unknown e2e test: ${TEST_NAME}" >&2
  echo "Available tests:" >&2
  find tests/e2e -maxdepth 1 -type f -name '*.sh' -exec basename {} .sh \; | sort >&2
  exit 1
fi
TEST_SCRIPT="$(realpath "${TEST_SCRIPT}")"

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
BIN_YARN="${REPO_ROOT}/target/release/yarn-bin"

if [[ ! -x "${BIN_YARN}" ]]; then
  echo "Missing compiled Yarn binary at ${BIN_YARN}" >&2
  exit 1
fi

TEMP_DIR="$(mktemp -d)"
declare -a BG_PIDS=()
SPAWN_BG_PID=""

cleanup() {
  local exit_code=$?
  if ((${#BG_PIDS[@]} > 0)); then
    for pid in "${BG_PIDS[@]}"; do
      kill "${pid}" 2>/dev/null || true
    done
    for pid in "${BG_PIDS[@]}"; do
      wait "${pid}" 2>/dev/null || true
    done
  fi
  rm -rf "${TEMP_DIR}"
  return "${exit_code}"
}
trap cleanup EXIT

function yarn() {
  "${BIN_YARN}" "$@"
}

PROJECT_DIR="${TEMP_DIR}/project"
mkdir -p "${PROJECT_DIR}"
cd "${PROJECT_DIR}"

function spawn_bg() {
  local log_file=""
  if [[ "${1:-}" == "--log" ]]; then
    if [[ $# -lt 3 ]]; then
      echo "Usage: spawn_bg --log <log-file> <command> [args...]" >&2
      return 1
    fi
    log_file="${2}"
    shift 2
  fi

  if [[ $# -eq 0 ]]; then
    echo "Usage: spawn_bg <command> [args...]" >&2
    return 1
  fi

  if [[ -n "${log_file}" ]]; then
    "$@" > "${log_file}" 2>&1 &
  else
    "$@" &
  fi

  local pid=$!
  BG_PIDS+=("${pid}")
  SPAWN_BG_PID="${pid}"
}

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
