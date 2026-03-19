import {ppath, xfs} from '@yarnpkg/fslib';

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
  describe(`tasks list`, () => {
    test(
      `it should return empty output when no long-lived tasks exist`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  echo "building"`,
        ].join(`\n`));

        await run(`install`);

        await runSwitch(`switch`, `daemon`, `--open`);
        await new Promise(resolve => setTimeout(resolve, 200));

        const {stdout} = await runSwitch(`tasks`, `--json`);
        expect(stdout).toBe(``);
      })),
    );

    test(
      `it should list a running long-lived task`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `@long-lived`,
          `server:`,
          `  echo "server-started"`,
          `  sleep 60`,
        ].join(`\n`));

        await run(`install`);

        // Start the long-lived task
        const serverPromise = runSwitch(`tasks`, `run`, `server`).catch(() => {});
        await new Promise(resolve => setTimeout(resolve, 800));

        const {stdout} = await runSwitch(`tasks`, `--json`);
        const lines = stdout.trim().split(`\n`);
        expect(lines).toHaveLength(1);

        expect(JSON.parse(lines[0])).toEqual({
          workspace: `test-package`,
          taskName: `server`,
          status: {
            running: {
              started_at_ms: expect.any(Number),
              process_id: expect.any(Number),
            },
          },
        });

        // Clean up
        await runSwitch(`tasks`, `stop`, `server`).catch(() => {});
      })),
    );

    test(
      `it should return empty output after a long-lived task is stopped`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `@long-lived`,
          `server:`,
          `  echo "server-started"`,
          `  sleep 60`,
        ].join(`\n`));

        await run(`install`);

        // Start and then stop the long-lived task
        const serverPromise = runSwitch(`tasks`, `run`, `server`).catch(() => {});
        await new Promise(resolve => setTimeout(resolve, 800));

        await runSwitch(`tasks`, `stop`, `server`);
        await new Promise(resolve => setTimeout(resolve, 500));

        // Stopped tasks are removed from the long-lived registry
        const {stdout} = await runSwitch(`tasks`, `--json`);
        expect(stdout).toBe(``);
      })),
    );

    test(
      `it should list multiple long-lived tasks`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `@long-lived`,
          `api:`,
          `  echo "api-started"`,
          `  sleep 60`,
          ``,
          `@long-lived`,
          `web:`,
          `  echo "web-started"`,
          `  sleep 60`,
        ].join(`\n`));

        await run(`install`);

        // Start both long-lived tasks sequentially to avoid race
        const apiPromise = runSwitch(`tasks`, `run`, `api`).catch(() => {});
        await new Promise(resolve => setTimeout(resolve, 800));

        const webPromise = runSwitch(`tasks`, `run`, `web`).catch(() => {});
        await new Promise(resolve => setTimeout(resolve, 800));

        const {stdout} = await runSwitch(`tasks`, `--json`);
        const lines = stdout.trim().split(`\n`);
        expect(lines).toHaveLength(2);

        const tasks = lines.map(l => JSON.parse(l));
        tasks.sort((a: any, b: any) => a.taskName.localeCompare(b.taskName));

        expect(tasks).toEqual([
          {
            workspace: `test-package`,
            taskName: `api`,
            status: {
              running: {
                started_at_ms: expect.any(Number),
                process_id: expect.any(Number),
              },
            },
          },
          {
            workspace: `test-package`,
            taskName: `web`,
            status: {
              running: {
                started_at_ms: expect.any(Number),
                process_id: expect.any(Number),
              },
            },
          },
        ]);

        // Clean up
        await runSwitch(`tasks`, `stop`, `api`).catch(() => {});
        await runSwitch(`tasks`, `stop`, `web`).catch(() => {});
      })),
    );
  });
});
