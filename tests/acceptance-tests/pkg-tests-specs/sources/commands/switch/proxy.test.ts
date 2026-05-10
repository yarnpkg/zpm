import {npath, ppath, xfs} from '@yarnpkg/fslib';
import {spawn}             from 'child_process';
import {delimiter}         from 'path';

import {RunFunction}       from '../../../../pkg-tests-core/sources/utils/tests';

function cleanupDaemon(cb: RunFunction): RunFunction {
  return async args => {
    try {
      await cb(args);
    } finally {
      await args.runSwitch(`switch`, `daemon`, `--kill-all`);
    }
  };
}

const getSwitchBinaryPath = () =>
  process.env.TEST_SWITCH_BINARY
    ?? require.resolve(`${__dirname}/../../../../../../target/release/yarn`);

const getYarnBinBinaryPath = () =>
  process.env.TEST_BINARY
    ?? require.resolve(`${__dirname}/../../../../../../target/release/yarn-bin`);

describe(`Commands`, () => {
  describe(`switch proxy`, () => {
    test(
      `it should exit with code 0 when a long-lived task receives SIGINT`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, yarnBinary}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `@long-lived`,
          `server:`,
          `  echo "server-started"`,
          `  sleep 60`,
        ].join(`\n`));

        await run(`install`);

        // Spawn the long-lived task
        const child = spawn(yarnBinary, [`tasks`, `run`, `server`], {
          cwd: npath.fromPortablePath(path),
          env: {...process.env},
          stdio: [`ignore`, `pipe`, `pipe`],
        });

        // Wait for the task to start
        await new Promise<void>((resolve, reject) => {
          const timeout = setTimeout(() => {
            reject(new Error(`Timeout waiting for server to start`));
          }, 10000);

          child.stdout?.on(`data`, (data: Buffer) => {
            if (data.toString().includes(`server-started`)) {
              clearTimeout(timeout);
              resolve();
            }
          });

          child.on(`error`, err => {
            clearTimeout(timeout);
            reject(err);
          });
        });

        // Send SIGINT to the child process
        child.kill(`SIGINT`);

        // Wait for the process to exit and check the exit code
        const exitCode = await new Promise<number | null>(resolve => {
          child.on(`close`, code => {
            resolve(code);
          });
        });

        // The process should exit with code 0, not 130 (SIGINT)
        expect(exitCode).toBe(0);
      })),
    );

    test(
      `it should delegate SIGINT to child when sent to the process group`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `signal-test.js`), [
          `process.on("SIGINT", () => {`,
          `  process.stdout.write("SIGINT handled\\n");`,
          `  process.exit(0);`,
          `});`,
          `process.stdout.write("ready\\n");`,
          `setInterval(() => {}, 1000);`,
        ].join(`\n`));

        await run(`install`);

        const nativePath = npath.fromPortablePath(path);
        const switchBinary = getSwitchBinaryPath();
        const yarnBinBinary = getYarnBinBinaryPath();

        const child = spawn(switchBinary, [`node`, `signal-test.js`], {
          cwd: nativePath,
          detached: true,
          stdio: [`ignore`, `pipe`, `pipe`],
          env: {
            HOME: npath.dirname(nativePath),
            PATH: `${nativePath}/bin${delimiter}${process.env.PATH}`,
            YARN_IS_TEST_ENV: `true`,
            YARN_GLOBAL_FOLDER: `${nativePath}/.yarn/global`,
            YARN_ENABLE_TELEMETRY: `0`,
            YARN_ENABLE_PROGRESS_BARS: `false`,
            YARN_ENABLE_TIMERS: `false`,
            FORCE_COLOR: `0`,
            NODE_OPTIONS: ``,
            YARNSW_DEFAULT: `local:${yarnBinBinary}`,
          },
        });

        await new Promise<void>((resolve, reject) => {
          const timeout = setTimeout(() => {
            reject(new Error(`Timeout waiting for child to be ready`));
          }, 10000);

          child.stdout?.on(`data`, (data: Buffer) => {
            if (data.toString().includes(`ready`)) {
              clearTimeout(timeout);
              resolve();
            }
          });

          child.on(`error`, err => {
            clearTimeout(timeout);
            reject(err);
          });
        });

        // Send SIGINT to the entire process group (simulates Ctrl-C)
        process.kill(-child.pid!, `SIGINT`);

        const exitCode = await new Promise<number | null>(resolve => {
          child.on(`close`, code => resolve(code));
        });

        expect(exitCode).toBe(0);
      }),
    );

    test(
      `it should delegate SIGTERM to child when sent to the process group`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `signal-test.js`), [
          `process.on("SIGTERM", () => {`,
          `  process.stdout.write("SIGTERM handled\\n");`,
          `  process.exit(0);`,
          `});`,
          `process.stdout.write("ready\\n");`,
          `setInterval(() => {}, 1000);`,
        ].join(`\n`));

        await run(`install`);

        const nativePath = npath.fromPortablePath(path);
        const switchBinary = getSwitchBinaryPath();
        const yarnBinBinary = getYarnBinBinaryPath();

        const child = spawn(switchBinary, [`node`, `signal-test.js`], {
          cwd: nativePath,
          detached: true,
          stdio: [`ignore`, `pipe`, `pipe`],
          env: {
            HOME: npath.dirname(nativePath),
            PATH: `${nativePath}/bin${delimiter}${process.env.PATH}`,
            YARN_IS_TEST_ENV: `true`,
            YARN_GLOBAL_FOLDER: `${nativePath}/.yarn/global`,
            YARN_ENABLE_TELEMETRY: `0`,
            YARN_ENABLE_PROGRESS_BARS: `false`,
            YARN_ENABLE_TIMERS: `false`,
            FORCE_COLOR: `0`,
            NODE_OPTIONS: ``,
            YARNSW_DEFAULT: `local:${yarnBinBinary}`,
          },
        });

        await new Promise<void>((resolve, reject) => {
          const timeout = setTimeout(() => {
            reject(new Error(`Timeout waiting for child to be ready`));
          }, 10000);

          child.stdout?.on(`data`, (data: Buffer) => {
            if (data.toString().includes(`ready`)) {
              clearTimeout(timeout);
              resolve();
            }
          });

          child.on(`error`, err => {
            clearTimeout(timeout);
            reject(err);
          });
        });

        // Send SIGTERM to the entire process group (simulates docker stop / tini)
        process.kill(-child.pid!, `SIGTERM`);

        const exitCode = await new Promise<number | null>(resolve => {
          child.on(`close`, code => resolve(code));
        });

        expect(exitCode).toBe(0);
      }),
    );
  });
});
