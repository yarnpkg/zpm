const {execFileSync} = require("node:child_process");
const path = require("node:path");

const args = process.argv.slice(2);

execFileSync(path.resolve(__dirname, "..", "target", "release", "yarn"), args, {
  env: {
    ...process.env,
    TEST_BINARY: process.env.TEST_BINARY,
  },
});