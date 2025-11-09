//! #!/usr/bin/env node
//
// This file is served by all endpoints such as:
// https://repo.yarnpkg.com/6.0.0/packages/yarnpkg-cli/bin/yarn.js
//
// Don't move it, otherwise you'll break them. Also keep in mind that Corepack will cache
// forever the versions it installs, so updates to this file will only take effect for people
// who don't have the file cached yet.

const {spawnSync} = require(`node:child_process`);
const fs = require(`node:fs`);
const path = require(`node:path`);
const os = require(`node:os`);

const yarnSwitchPath = path.join(os.homedir(), `.yarn`, `switch`, `bin`, `yarn`);

function main() {
  if (!process.env.YARNSW_COREPACK_COMPAT) {
    console.log(`Corepack doesn't currently support Yarn versions 6.0 and higher, due to them`);
    console.log(`being distributed as binaries.`);
    console.log();
    console.log(`Official Yarn guidelines now recommend using Yarn Switch, a tool maintained by`);
    console.log(`the Yarn project; check our website for more information: https://yarnpkg.com`);
    console.log();
    console.log(`You can install Yarn Switch by running:`);
    console.log(`curl -s https://repo.yarnpkg.com/install | bash`);
    console.log();
    console.log(`If running within a restricted environment you can temporarily bypass this`);
    console.log(`check by setting the YARNSW_COREPACK_COMPAT environment variable to true. It`);
    console.log(`will instruct this script to install Yarn Switch and shell out to it.`);

    process.exitCode = 1;
    return;
  }

  if (!fs.existsSync(yarnSwitchPath)) {
    const switchResult = spawnSync(`bash -c "set -euo pipefail; curl -s https://repo.yarnpkg.com/install | bash"`, {
      stdio: `pipe`,
      shell: true,
    });

    if (switchResult.error) {
      console.log(switchResult.error.toString());
    } else if (switchResult.status !== 0) {
      console.log(`stdout: ${switchResult.stdout.toString().trim()}`);
      console.log(`stderr: ${switchResult.stderr.toString().trim()}`);
    }

    if (switchResult.status !== 0) {
      console.log();
      console.log(`Failed to install Yarn Switch; run the following command to install it:`);
      console.log(`curl -s https://repo.yarnpkg.com/install | bash`);

      process.exitCode = 1;
      return;
    }
  }

  const yarnResult = spawnSync(yarnSwitchPath, process.argv.slice(2), {
    stdio: `inherit`,
    env: {
      ...process.env,
      PATH: `${path.dirname(yarnSwitchPath)}${path.delimiter}${process.env.PATH}`,
    },
  });

  function sigintHandler() {
    // We don't want SIGINT to kill our process; we want it to kill the
    // innermost process, whose end will cause our own to exit.
  }

  function sigtermHandler() {
    yarnResult.kill();
  }

  process.on(`SIGINT`, sigintHandler);
  process.on(`SIGTERM`, sigtermHandler);

  if (typeof yarnResult.status === `number`) {
    process.exitCode = yarnResult.status;
  } else {
    process.exitCode = 1;
  }
}

main();
