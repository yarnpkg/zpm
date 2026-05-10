#!/usr/bin/env bash

npm install -g pnpm

mkdir -p packages/pkg-a

cat > package.json <<'JSON'
{
  "name": "e2e-pnpm-migration",
  "private": true,
  "workspaces": [
    "packages/*"
  ]
}
JSON

cat > pnpm-workspace.yaml <<'YAML'
packages:
  - "packages/*"
YAML

cat > packages/pkg-a/package.json <<'JSON'
{
  "name": "pkg-a",
  "version": "1.0.0",
  "private": true,
  "dependencies": {
    "jest": "*"
  }
}
JSON

pnpm install

# Capture the versions pnpm resolved (normalized to name@version)
pnpm ls -r --json --depth=Infinity \
  | jq -r '.. | objects | select(.version? and .from?) | "\(.from)@\(.version)"' \
  | sort -u > "${TEMP_DIR}/pnpm-versions.txt"

rm -f pnpm-lock.yaml

yarn install

# Capture the versions yarn resolved (normalized to name@version)
yarn info -AR --json \
  | jq -r '.value' \
  | grep '@npm:' \
  | sed -E 's/@npm:(.+@)?/\t/' \
  | awk -F'\t' '{print $1"@"$2}' \
  | sort -u > "${TEMP_DIR}/yarn-versions.txt"

# Every version from pnpm must appear in yarn's resolution.
# Platform-specific packages (like fsevents) may be absent; that's expected.
MISMATCHES=0
while IFS= read -r line; do
  if ! grep -qxF "${line}" "${TEMP_DIR}/yarn-versions.txt"; then
    echo "MISMATCH: pnpm had ${line} but yarn does not" >&2
    MISMATCHES=$((MISMATCHES + 1))
  fi
done < "${TEMP_DIR}/pnpm-versions.txt"

if [[ "${MISMATCHES}" -gt 5 ]]; then
  echo "Too many version mismatches (${MISMATCHES}); migration did not preserve versions" >&2
  exit 1
fi
