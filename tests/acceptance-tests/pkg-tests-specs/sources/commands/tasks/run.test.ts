import {ppath, xfs}  from '@yarnpkg/fslib';

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
  describe(`tasks run`, () => {
    test(
      `it should run a simple task`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  echo "building"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `build`);
        expect(stdout).toEqual(`building\n`);
      })),
    );

    test(
      `it should run a task with dependencies in order`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `setup:`,
          `  echo "setup"`,
          ``,
          `build: setup`,
          `  echo "build"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `build`);
        expect(stdout).toEqual(`setup\nbuild\n`);
      })),
    );

    test(
      `it should show prefixes with verbose level 1`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  echo "building"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `-v`, `build`);
        expect(stdout).toEqual(`[test-package:build]: building\n`);
      })),
    );

    test(
      `it should show prologue and epilogue with verbose level 2`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  echo "building"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `-vv`, `build`);
        expect(stdout).toEqual(`[test-package:build]: Process started\n[test-package:build]: building\n[test-package:build]: Process exited (exit code 0)\n`);
      })),
    );

    test(
      `it should hide dependency output with --silent-dependencies`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `setup:`,
          `  echo "setup-output"`,
          ``,
          `build: setup`,
          `  echo "build-output"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `--silent-dependencies`, `build`);
        expect(stdout).toEqual(`build-output\n`);
      })),
    );

    test(
      `it should show dependency output on failure even with --silent-dependencies`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `setup:`,
          `  echo "setup-failure-output"`,
          `  exit 1`,
          ``,
          `build: setup`,
          `  echo "build-output"`,
        ].join(`\n`));

        await run(`install`);

        await expect(runSwitch(`tasks`, `run`, `--silent-dependencies`, `build`)).rejects.toMatchObject({
          stdout: `[test-package:setup]: Process started\n[test-package:setup]: setup-failure-output\n[test-package:setup]: Process exited (exit code 1)\n`,
          code: 1,
        });
      })),
    );

    test(
      `it should forward yarn run to task run with silent dependencies`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `setup:`,
          `  echo "setup-output"`,
          ``,
          `build: setup`,
          `  echo "build-output"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await runSwitch(`run`, `build`);
        expect(stdout).toEqual(`build-output\n`);
      })),
    );

    test(
      `it should forward yarn run to task run and show verbose output on failure`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `setup:`,
          `  echo "setup-failure-output"`,
          `  exit 1`,
          ``,
          `build: setup`,
          `  echo "build-output"`,
        ].join(`\n`));

        await run(`install`);

        await expect(runSwitch(`run`, `build`)).rejects.toMatchObject({
          stdout: `[test-package:setup]: Process started\n[test-package:setup]: setup-failure-output\n[test-package:setup]: Process exited (exit code 1)\n`,
          code: 1,
        });
      })),
    );

    test(
      `it should pass arguments to the target task`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `greet:`,
          `  echo "Hello $1"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `greet`, `World`);
        expect(stdout).toEqual(`Hello World\n`);
      })),
    );

    test(
      `it should fail when the task does not exist`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  echo "building"`,
        ].join(`\n`));

        await run(`install`);

        await expect(runSwitch(`tasks`, `run`, `nonexistent`)).rejects.toMatchObject({
          code: 1,
        });
      })),
    );

    test(
      `it should fail when there is no taskfile`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await run(`install`);

        await expect(runSwitch(`tasks`, `run`, `build`)).rejects.toMatchObject({
          code: 1,
        });
      })),
    );

    test(
      `it should run parallel dependencies concurrently`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `task-a:`,
          `  sleep 0.1 && echo "task-a"`,
          ``,
          `task-b:`,
          `  sleep 0.2 && echo "task-b"`,
          ``,
          `build: task-a& task-b&`,
          `  echo "build"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `build`);

        const lines = stdout.trim().split(`\n`);
        expect(lines).toHaveLength(3);
        expect([lines[0], lines[1]].sort()).toEqual([`task-a`, `task-b`]);
        expect(lines[2]).toEqual(`build`);
      })),
    );

    test(
      `it should run tasks across workspaces`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`},
        [`packages/pkg-b`]: {name: `pkg-b`, dependencies: {[`pkg-a`]: `workspace:*`}},
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-a/taskfile` as any), [
          `build:`,
          `  echo "building-pkg-a"`,
        ].join(`\n`));

        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/taskfile` as any), [
          `build: pkg-a:build`,
          `  echo "building-pkg-b"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `build`, {cwd: ppath.join(path, `packages/pkg-b` as any)});
        expect(stdout).toEqual(`building-pkg-a\nbuilding-pkg-b\n`);
      })),
    );

    test(
      `it should hide cross-workspace dependency output with --silent-dependencies`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`},
        [`packages/pkg-b`]: {name: `pkg-b`, dependencies: {[`pkg-a`]: `workspace:*`}},
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-a/taskfile` as any), [
          `build:`,
          `  echo "building-pkg-a"`,
        ].join(`\n`));

        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/taskfile` as any), [
          `build: pkg-a:build`,
          `  echo "building-pkg-b"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `--silent-dependencies`, `build`, {cwd: ppath.join(path, `packages/pkg-b` as any)});
        expect(stdout).toEqual(`building-pkg-b\n`);
      })),
    );

    test(
      `it should hide pushed subtask output with --silent-dependencies`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `subtask:`,
          `  echo "subtask-output"`,
          ``,
          `main:`,
          `  yarn tasks push subtask`,
          `  echo "main-output"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `--silent-dependencies`, `main`);
        expect(stdout).toEqual(`main-output\n`);
      })),
    );

    test(
      `it should return the exit code of the failed task`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  exit 42`,
        ].join(`\n`));

        await run(`install`);

        await expect(runSwitch(`tasks`, `run`, `build`)).rejects.toMatchObject({
          code: 42,
        });
      })),
    );

    test(
      `it should re-run the same task when called multiple times`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        const counterFile = ppath.join(path, `counter`);

        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  count=$(cat counter 2>/dev/null || echo 0)`,
          `  count=$((count + 1))`,
          `  echo $count > counter`,
          `  echo "run $count"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout: stdout1} = await runSwitch(`tasks`, `run`, `build`);
        expect(stdout1).toEqual(`run 1\n`);

        const {stdout: stdout2} = await runSwitch(`tasks`, `run`, `build`);
        expect(stdout2).toEqual(`run 2\n`);

        const {stdout: stdout3} = await runSwitch(`tasks`, `run`, `build`);
        expect(stdout3).toEqual(`run 3\n`);
      })),
    );

    test(
      `it should stream log lines in real-time`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        // Create a task that outputs lines with delays and includes script-side timestamps
        // Use Python for cross-platform millisecond timestamps
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `stream-test:`,
          `  python3 -c "import time; print(f'ts:{int(time.time()*1000)}:line1')"`,
          `  sleep 0.5`,
          `  python3 -c "import time; print(f'ts:{int(time.time()*1000)}:line2')"`,
          `  sleep 0.5`,
          `  python3 -c "import time; print(f'ts:{int(time.time()*1000)}:line3')"`,
        ].join(`\n`));

        await run(`install`);

        // Measure total execution time
        const startTime = Date.now();
        const {stdout} = await runSwitch(`tasks`, `run`, `stream-test`);
        const endTime = Date.now();
        const totalTime = endTime - startTime;

        console.log(stdout);

        // Parse timestamps from script output
        // Format: ts:1234567890123:lineN
        const timestampRegex = /^ts:(\d+):(.+)$/;
        const lines = stdout.trim().split(`\n`);

        expect(lines.length).toBe(3);

        const timestamps: Array<number> = [];
        const messages: Array<string> = [];

        for (const line of lines) {
          const match = line.match(timestampRegex);
          expect(match).not.toBeNull();
          if (match) {
            timestamps.push(parseInt(match[1], 10));
            messages.push(match[2]);
          }
        }

        // Verify the messages are correct
        expect(messages).toEqual([`line1`, `line2`, `line3`]);

        // Verify script-side timestamps are properly spaced (at least 400ms apart)
        // This proves the script's sleep commands executed between echo statements
        for (let i = 1; i < timestamps.length; i++) {
          const diff = timestamps[i] - timestamps[i - 1];
          expect(diff).toBeGreaterThanOrEqual(400);
        }

        // Verify total execution time is reasonable (at least 900ms for two 500ms sleeps)
        // This proves output wasn't queued and released at the end
        expect(totalTime).toBeGreaterThanOrEqual(900);
      })),
    );
  });
});
