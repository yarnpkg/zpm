#!/usr/bin/env bash

cat > package.json <<'JSON'
{
  "name": "e2e-smoke",
  "private": true,
  "dependencies": {
    "is-number": "^7.0.0"
  }
}
JSON

yarn install
yarn node -e "const isNumber=require('is-number'); if(!isNumber(1)||isNumber('x')) process.exit(1);"
