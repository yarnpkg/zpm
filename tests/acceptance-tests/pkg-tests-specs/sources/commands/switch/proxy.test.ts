import {npath, ppath, xfs} from '@yarnpkg/fslib';
import {spawn} from 'child_process';

import {RunFunction} from '../../../../pkg-tests-core/sources/utils/tests';

function cleanupDaemon(cb: RunFunction): RunFunction {
  return async args => {
    try {
      await cb(args);
    } finally {
      await args.runSwitch(`switch`, `daemon`, `--kill-all`);
    }
  };
}

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

          child.on(`error`, (err) => {
            clearTimeout(timeout);
            reject(err);
          });
        });

        // Send SIGINT to the child process
        child.kill(`SIGINT`);

        // Wait for the process to exit and check the exit code
        const exitCode = await new Promise<number | null>((resolve) => {
          child.on(`close`, (code) => {
            resolve(code);
          });
        });

        // The process should exit with code 0, not 130 (SIGINT)
        expect(exitCode).toBe(0);
      })),
    );
  });
});
