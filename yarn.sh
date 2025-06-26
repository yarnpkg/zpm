#!/usr/bin/env bash

set -e

REPO_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" &> /dev/null && realpath .)

ZPM_BUILD=${ZPM_BUILD:-release}

ZPM_SWITCH_BINARY_PATH="${REPO_DIR}/target/${ZPM_BUILD}/yarn"
ZPM_BINARY_PATH="${REPO_DIR}/target/${ZPM_BUILD}/yarn-bin"

# So that Yarn Switch knows it should load a local binary; this requires
# that the repository doesn't have a `packageManager` field set up.
export YARNSW_DEFAULT="local:${ZPM_BINARY_PATH}"

# So that the test runner from the Yarn Berry repository knows where to
# find the zpm test binary. It needs to be a JS file.
export TEST_BINARY="${REPO_DIR}/yarn.sh"

# To disable tests that we don't want to run.
export TEST_MAJOR="5"

$ZPM_SWITCH_BINARY_PATH "$@"
