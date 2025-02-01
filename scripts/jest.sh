#!/usr/bin/env bash

set -e

REPO_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" &> /dev/null && realpath ..)
BERRY_DIR=${BERRY_PATH:-~/berry}

export TEST_BINARY=${REPO_DIR}/scripts/exec-${TEST_BINARY:-release}.mjs

cd ${BERRY_DIR}
yarn test:integration "$@"
