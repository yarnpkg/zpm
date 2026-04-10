#!/usr/bin/env bash
set -euo pipefail

TEST_SCRIPT="${1:-}"
LOG_FILE="${2:-}"
WORKSPACE="${GITHUB_WORKSPACE:-/github/workspace}"

if [[ -z "${TEST_SCRIPT}" ]]; then
  echo "Missing required argument: test-script" >&2
  echo "exit-code=2" >> "${GITHUB_OUTPUT}"
  exit 0
fi

if [[ -z "${LOG_FILE}" ]]; then
  echo "Missing required argument: log-file" >&2
  echo "exit-code=2" >> "${GITHUB_OUTPUT}"
  exit 0
fi

if [[ "${LOG_FILE}" != /* ]]; then
  LOG_FILE="${WORKSPACE}/${LOG_FILE}"
fi

mkdir -p "$(dirname "${LOG_FILE}")"
export HOME="${HOME:-/tmp/e2e-home}"
mkdir -p "${HOME}"

cd "${WORKSPACE}"

if [[ -f "${WORKSPACE}/target/release/yarn" ]]; then
  chmod +x "${WORKSPACE}/target/release/yarn"
fi

if [[ -f "${WORKSPACE}/target/release/yarn-bin" ]]; then
  chmod +x "${WORKSPACE}/target/release/yarn-bin"
fi

chmod +x "${WORKSPACE}/${TEST_SCRIPT}"

set +e
"${WORKSPACE}/${TEST_SCRIPT}" 2>&1 | tee "${LOG_FILE}"
exit_code=${PIPESTATUS[0]}
set -e

echo "exit-code=${exit_code}" >> "${GITHUB_OUTPUT}"
exit 0
