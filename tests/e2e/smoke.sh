#!/usr/bin/env bash
set -exou pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
YARN_BIN="${REPO_ROOT}/target/release/yarn-bin"

if [[ ! -x "${YARN_BIN}" ]]; then
  echo "Missing compiled Yarn binary at ${YARN_BIN}" >&2
  exit 1
fi

WORKDIR="$(mktemp -d)"
cleanup() {
  rm -rf "${WORKDIR}"
}
trap cleanup EXIT

mkdir -p "${WORKDIR}/bin"
ln -sf "${YARN_BIN}" "${WORKDIR}/bin/yarn"
export PATH="${WORKDIR}/bin:${PATH}"

PROJECT_DIR="${WORKDIR}/project"
mkdir -p "${PROJECT_DIR}"
cd "${PROJECT_DIR}"

cat > package.json <<'JSON'
{
  "name": "e2e-smoke",
  "private": true,
  "packageManager": "yarn@6.0.0-rc.10",
  "dependencies": {
    "is-number": "^7.0.0"
  }
}
JSON

yarn install
yarn node -e "const isNumber=require('is-number'); if(!isNumber(1)||isNumber('x')) process.exit(1);"
