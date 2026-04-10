#!/usr/bin/env bash
set -euo pipefail

TEST_NAME="${1:-}"
LOG_FILE="${2:-}"
WORKSPACE="${GITHUB_WORKSPACE:-/github/workspace}"

if [[ -z "${TEST_NAME}" ]]; then
  echo "Missing required argument: test-name" >&2
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
chmod +x "${WORKSPACE}/tests/e2e/${TEST_NAME}.sh"

set +e
bash "${WORKSPACE}/scripts/e2e/run.sh" "${TEST_NAME}" 2>&1 | tee "${LOG_FILE}"
exit_code=${PIPESTATUS[0]}
set -e

echo "exit-code=${exit_code}" >> "${GITHUB_OUTPUT}"
exit 0
