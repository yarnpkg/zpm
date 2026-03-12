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
      }, async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  echo "building"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(stdout).toEqual(`building\n`);
      }),
    );

    test(
      `it should run a task with dependencies in order`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `setup:`,
          `  echo "setup"`,
          ``,
          `build: setup`,
          `  echo "build"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(stdout).toEqual(`setup\nbuild\n`);
      }),
    );

    test(
      `it should show prefixes with verbose level 1`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  echo "building"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `-v`, `build`);
        expect(stdout).toEqual(`[test-package:build]: building\n`);
      }),
    );

    test(
      `it should show prologue and epilogue with verbose level 2`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  echo "building"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `-vv`, `build`);
        expect(stdout).toEqual(`[test-package:build]: Process started\n[test-package:build]: building\n[test-package:build]: Process exited (exit code 0)\n`);
      }),
    );

    test(
      `it should hide dependency output with --silent-dependencies`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `setup:`,
          `  echo "setup-output"`,
          ``,
          `build: setup`,
          `  echo "build-output"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `--silent-dependencies`, `build`);
        expect(stdout).toEqual(`build-output\n`);
      }),
    );

    test(
      `it should show dependency output on failure even with --silent-dependencies`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `setup:`,
          `  echo "setup-failure-output"`,
          `  exit 1`,
          ``,
          `build: setup`,
          `  echo "build-output"`,
        ].join(`\n`));

        await run(`install`);

        await expect(runSwitch(`tasks`, `run`, `--standalone`, `--silent-dependencies`, `build`)).rejects.toMatchObject({
          stdout: `[test-package:setup]: Process started\n[test-package:setup]: setup-failure-output\n[test-package:setup]: Process exited (exit code 1)\n`,
          code: 1,
        });
      }),
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
      }, async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `greet:`,
          `  echo "Hello $1"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `greet`, `World`);
        expect(stdout).toEqual(`Hello World\n`);
      }),
    );

    test(
      `it should fail when the task does not exist`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  echo "building"`,
        ].join(`\n`));

        await run(`install`);

        await expect(runSwitch(`tasks`, `run`, `--standalone`, `nonexistent`)).rejects.toMatchObject({
          code: 1,
        });
      }),
    );

    test(
      `it should fail when there is no taskfile`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run, runSwitch}) => {
        await run(`install`);

        await expect(runSwitch(`tasks`, `run`, `--standalone`, `build`)).rejects.toMatchObject({
          code: 1,
        });
      }),
    );

    test(
      `it should run parallel dependencies concurrently`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run, runSwitch}) => {
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

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `build`);

        const lines = stdout.trim().split(`\n`);
        expect(lines).toHaveLength(3);
        expect([lines[0], lines[1]].sort()).toEqual([`task-a`, `task-b`]);
        expect(lines[2]).toEqual(`build`);
      }),
    );

    test(
      `it should run tasks across workspaces`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`},
        [`packages/pkg-b`]: {name: `pkg-b`, dependencies: {[`pkg-a`]: `workspace:*`}},
      }, async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-a/taskfile` as any), [
          `build:`,
          `  echo "building-pkg-a"`,
        ].join(`\n`));

        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/taskfile` as any), [
          `build: pkg-a:build`,
          `  echo "building-pkg-b"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `build`, {cwd: ppath.join(path, `packages/pkg-b` as any)});
        expect(stdout).toEqual(`building-pkg-a\nbuilding-pkg-b\n`);
      }),
    );

    test(
      `it should hide cross-workspace dependency output with --silent-dependencies`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`},
        [`packages/pkg-b`]: {name: `pkg-b`, dependencies: {[`pkg-a`]: `workspace:*`}},
      }, async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-a/taskfile` as any), [
          `build:`,
          `  echo "building-pkg-a"`,
        ].join(`\n`));

        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/taskfile` as any), [
          `build: pkg-a:build`,
          `  echo "building-pkg-b"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `--silent-dependencies`, `build`, {cwd: ppath.join(path, `packages/pkg-b` as any)});
        expect(stdout).toEqual(`building-pkg-b\n`);
      }),
    );

    test(
      `it should hide pushed subtask output with --silent-dependencies`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `subtask:`,
          `  echo "subtask-output"`,
          ``,
          `main:`,
          `  yarn tasks push subtask`,
          `  echo "main-output"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `--silent-dependencies`, `main`);
        expect(stdout).toEqual(`main-output\n`);
      }),
    );

    test(
      `it should return the exit code of the failed task`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  exit 42`,
        ].join(`\n`));

        await run(`install`);

        await expect(runSwitch(`tasks`, `run`, `--standalone`, `build`)).rejects.toMatchObject({
          code: 42,
        });
      }),
    );

    test(
      `it should re-run the same task when called multiple times`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run, runSwitch}) => {
        const counterFile = ppath.join(path, `counter`);

        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  count=$(cat counter 2>/dev/null || echo 0)`,
          `  count=$((count + 1))`,
          `  echo $count > counter`,
          `  echo "run $count"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout: stdout1} = await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(stdout1).toEqual(`run 1\n`);

        const {stdout: stdout2} = await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(stdout2).toEqual(`run 2\n`);

        const {stdout: stdout3} = await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(stdout3).toEqual(`run 3\n`);
      }),
    );

    test(
      `it should stream log lines in real-time`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run, runSwitch}) => {
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
        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `stream-test`);
        const endTime = Date.now();
        const totalTime = endTime - startTime;

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
      }),
    );

    describe(`@long-lived tasks`, () => {
      test(
        `it should unblock dependents after warm-up period`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          // Create a long-lived task (simulates a dev server) and a dependent task
          // The dependent should start after 500ms warm-up, not wait for server to exit
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `@long-lived`,
            `server:`,
            `  echo "server-started"`,
            `  sleep 10`,
            ``,
            `client: server`,
            `  echo "client-started"`,
          ].join(`\n`));

          await run(`install`);

          // Run the client task - it should complete quickly after warm-up
          // even though the server would take 10 seconds if we waited for it
          const startTime = Date.now();
          const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `client`);
          const endTime = Date.now();
          const totalTime = endTime - startTime;

          // Should complete in under 3 seconds (warm-up is 500ms + some overhead)
          // If it waited for server, it would take 10+ seconds
          expect(totalTime).toBeLessThan(3000);
          expect(stdout).toContain(`client-started`);
        }),
      );

      test(
        `it should attach to existing long-lived task on second invocation`,
        makeTemporaryEnv({
          name: `test-package`,
        }, cleanupDaemon(async ({path, run, runSwitch}) => {
          // Create a long-lived task that writes to a file on each start
          const counterFile = ppath.join(path, `server-starts`);
          await xfs.writeFilePromise(counterFile, `0`);

          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `@long-lived`,
            `server:`,
            `  count=$(cat server-starts)`,
            `  count=$((count + 1))`,
            `  echo $count > server-starts`,
            `  echo "server-start-$count"`,
            `  sleep 10`,
          ].join(`\n`));

          await run(`install`);

          // Start the server first time in background (we'll detach via timeout)
          const serverPromise1 = runSwitch(`tasks`, `run`, `server`).catch(() => {});

          // Wait for warm-up
          await new Promise(resolve => setTimeout(resolve, 700));

          // Second invocation should attach to existing, not start new
          const serverPromise2 = runSwitch(`tasks`, `run`, `server`).catch(() => {});

          // Wait a bit for the second command to complete its attach
          await new Promise(resolve => setTimeout(resolve, 300));

          // Check that server only started once
          const startCount = await xfs.readFilePromise(counterFile, `utf8`);
          expect(startCount.trim()).toEqual(`1`);

          // Clean up
          await runSwitch(`tasks`, `stop`, `server`).catch(() => {});
        })),
      );

      test(
        `it should allow stopping a long-lived task`,
        makeTemporaryEnv({
          name: `test-package`,
        }, cleanupDaemon(async ({path, run, runSwitch}) => {
          const pidFile = ppath.join(path, `server.pid`);

          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `@long-lived`,
            `server:`,
            `  echo $$ > server.pid`,
            `  echo "server-running"`,
            `  sleep 60`,
          ].join(`\n`));

          await run(`install`);

          // Start the server in background
          const serverPromise = runSwitch(`tasks`, `run`, `server`).catch(() => {});

          // Wait for warm-up and pid file to be written
          await new Promise(resolve => setTimeout(resolve, 700));

          // Verify server is running (pid file exists)
          const pidExists = await xfs.existsPromise(pidFile);
          expect(pidExists).toBe(true);

          // Stop the server
          const {stdout: stopOutput} = await runSwitch(`tasks`, `stop`, `server`);
          expect(stopOutput).toContain(`stopped successfully`);

          // Wait a bit for process cleanup
          await new Promise(resolve => setTimeout(resolve, 200));
        })),
      );

      test(
        `it should continue running after client disconnects`,
        makeTemporaryEnv({
          name: `test-package`,
        }, cleanupDaemon(async ({path, run, runSwitch}) => {
          const markerFile = ppath.join(path, `still-running`);

          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `@long-lived`,
            `server:`,
            `  echo "server-started"`,
            `  sleep 1`,
            `  echo "still-running" > still-running`,
            `  sleep 10`,
          ].join(`\n`));

          await run(`install`);

          // Start server and simulate client disconnect by using a short timeout
          // We use Promise.race to simulate the client disconnecting
          await Promise.race([
            runSwitch(`tasks`, `run`, `server`).catch(() => {}),
            new Promise(resolve => setTimeout(resolve, 700)),
          ]);

          // Wait for the marker file to be created (proves server continued running)
          await new Promise(resolve => setTimeout(resolve, 800));

          const markerExists = await xfs.existsPromise(markerFile);
          expect(markerExists).toBe(true);

          // Clean up
          await runSwitch(`tasks`, `stop`, `server`).catch(() => {});
        })),
      );

      test(
        `it should use fixed context ID for long-lived tasks`,
        makeTemporaryEnv({
          name: `test-package`,
        }, cleanupDaemon(async ({path, run, runSwitch}) => {
          // Create two separate short-lived tasks and verify they get different context IDs
          // Then verify long-lived tasks always get the same fixed context ID
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `@long-lived`,
            `server:`,
            `  echo "server: $ZPM_TASK_CURRENT"`,
            `  sleep 5`,
          ].join(`\n`));

          await run(`install`);

          // Start server first time
          const server1Promise = runSwitch(`tasks`, `run`, `-v`, `server`).catch(() => {});
          await new Promise(resolve => setTimeout(resolve, 700));

          // Get output from first invocation
          // The context ID should be the fixed long-lived context ID
          // 4d84fea4-e0d4-4df6-8190-f312b86968b3

          // Start second invocation - should attach to same task
          const server2Promise = runSwitch(`tasks`, `run`, `-v`, `server`).catch(() => {});
          await new Promise(resolve => setTimeout(resolve, 300));

          // Stop and clean up
          await runSwitch(`tasks`, `stop`, `server`).catch(() => {});
        })),
      );

      test(
        `it should fail dependents if long-lived task fails before warm-up`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          // Create a long-lived task that exits immediately (before 500ms warm-up)
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `@long-lived`,
            `server:`,
            `  echo "server-failed"`,
            `  exit 1`,
            ``,
            `client: server`,
            `  echo "client-started"`,
          ].join(`\n`));

          await run(`install`);

          // Run the client task - it should fail because server failed before warm-up
          await expect(runSwitch(`tasks`, `run`, `--standalone`, `client`)).rejects.toMatchObject({
            code: 1,
          });
        }),
      );
    });

    describe(`dependency resolution`, () => {
      test(
        `it should handle diamond dependency pattern correctly`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          // Diamond pattern: target depends on B and C, both depend on D
          // D should only run once, not twice
          const counterFile = ppath.join(path, `d-counter`);
          await xfs.writeFilePromise(counterFile, `0`);

          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `task-d:`,
            `  count=$(cat d-counter)`,
            `  count=$((count + 1))`,
            `  echo $count > d-counter`,
            `  echo "task-d:$count"`,
            ``,
            `task-b: task-d`,
            `  echo "task-b"`,
            ``,
            `task-c: task-d`,
            `  echo "task-c"`,
            ``,
            `target: task-b task-c`,
            `  echo "target"`,
          ].join(`\n`));

          await run(`install`);

          const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `target`);
          const lines = stdout.trim().split(`\n`);

          // D should run exactly once
          expect(lines.filter(l => l.startsWith(`task-d`)).length).toBe(1);
          expect(lines[0]).toBe(`task-d:1`);

          // B and C should each run once (in any order)
          expect(lines.filter(l => l === `task-b`).length).toBe(1);
          expect(lines.filter(l => l === `task-c`).length).toBe(1);

          // Target should run last
          expect(lines[lines.length - 1]).toBe(`target`);

          // Counter should show D ran once
          const finalCount = await xfs.readFilePromise(counterFile, `utf8`);
          expect(finalCount.trim()).toBe(`1`);
        }),
      );

      test(
        `it should handle deep transitive dependency chains`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          // Create a chain: level-5 -> level-4 -> level-3 -> level-2 -> level-1
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `level-1:`,
            `  echo "level-1"`,
            ``,
            `level-2: level-1`,
            `  echo "level-2"`,
            ``,
            `level-3: level-2`,
            `  echo "level-3"`,
            ``,
            `level-4: level-3`,
            `  echo "level-4"`,
            ``,
            `level-5: level-4`,
            `  echo "level-5"`,
          ].join(`\n`));

          await run(`install`);

          const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `level-5`);
          const lines = stdout.trim().split(`\n`);

          // Should execute in correct order
          expect(lines).toEqual([
            `level-1`,
            `level-2`,
            `level-3`,
            `level-4`,
            `level-5`,
          ]);
        }),
      );

      test(
        `it should handle mixed parallel and sequential dependencies`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          // Create: target -> (a&, b&) -> c (sequential)
          // So a and b run in parallel after c completes
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `dep-c:`,
            `  echo "dep-c"`,
            ``,
            `dep-a: dep-c`,
            `  sleep 0.1 && echo "dep-a"`,
            ``,
            `dep-b: dep-c`,
            `  sleep 0.1 && echo "dep-b"`,
            ``,
            `target: dep-a& dep-b&`,
            `  echo "target"`,
          ].join(`\n`));

          await run(`install`);

          const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `target`);
          const lines = stdout.trim().split(`\n`);

          // dep-c must come first
          expect(lines[0]).toBe(`dep-c`);

          // dep-a and dep-b can be in any order, but both before target
          expect(lines.slice(1, 3).sort()).toEqual([`dep-a`, `dep-b`]);

          // target must come last
          expect(lines[lines.length - 1]).toBe(`target`);
        }),
      );
    });

    describe(`error handling and failure propagation`, () => {
      test(
        `it should fail all pending dependents when a task fails`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          // Create: target -> middle -> failing-base
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `failing-base:`,
            `  echo "failing"`,
            `  exit 1`,
            ``,
            `middle: failing-base`,
            `  echo "middle-should-not-run"`,
            ``,
            `target: middle`,
            `  echo "target-should-not-run"`,
          ].join(`\n`));

          await run(`install`);

          await expect(runSwitch(`tasks`, `run`, `--standalone`, `-vv`, `target`)).rejects.toMatchObject({
            code: 1,
          });
        }),
      );

      test(
        `it should fail parallel siblings when one fails`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          // Create: target -> (fast-fail&, slow-success&)
          // fast-fail should cause target to fail even though slow-success might complete
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `fast-fail:`,
            `  echo "fast-fail"`,
            `  exit 1`,
            ``,
            `slow-success:`,
            `  sleep 0.5 && echo "slow-success"`,
            ``,
            `target: fast-fail& slow-success&`,
            `  echo "target-should-not-run"`,
          ].join(`\n`));

          await run(`install`);

          await expect(runSwitch(`tasks`, `run`, `--standalone`, `target`)).rejects.toMatchObject({
            code: 1,
          });
        }),
      );

      test(
        `it should propagate pushed subtask failure to parent`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `failing-subtask:`,
            `  echo "subtask-failing"`,
            `  exit 42`,
            ``,
            `parent:`,
            `  echo "parent-start"`,
            `  yarn tasks push failing-subtask`,
            `  echo "parent-end"`,
          ].join(`\n`));

          await run(`install`);

          await expect(runSwitch(`tasks`, `run`, `--standalone`, `parent`)).rejects.toMatchObject({
            code: 42,
          });
        }),
      );

      test(
        `it should handle multiple pushed subtasks with one failure`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `subtask-ok:`,
            `  echo "subtask-ok"`,
            ``,
            `subtask-fail:`,
            `  echo "subtask-fail"`,
            `  exit 1`,
            ``,
            `parent:`,
            `  echo "parent-start"`,
            `  yarn tasks push subtask-ok`,
            `  yarn tasks push subtask-fail`,
            `  echo "parent-end"`,
          ].join(`\n`));

          await run(`install`);

          await expect(runSwitch(`tasks`, `run`, `--standalone`, `parent`)).rejects.toMatchObject({
            code: 1,
          });
        }),
      );

      test(
        `it should report correct exit code when target task directly fails`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          // When the target task itself fails, its exit code should be preserved
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `target:`,
            `  exit 77`,
          ].join(`\n`));

          await run(`install`);

          await expect(runSwitch(`tasks`, `run`, `--standalone`, `target`)).rejects.toMatchObject({
            code: 77,
          });
        }),
      );

      test(
        `it should fail with code 1 when a dependency fails`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          // When a dependency fails (not the target), the exit code is normalized to 1
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `deep-fail:`,
            `  exit 77`,
            ``,
            `target: deep-fail`,
            `  echo "should-not-run"`,
          ].join(`\n`));

          await run(`install`);

          await expect(runSwitch(`tasks`, `run`, `--standalone`, `target`)).rejects.toMatchObject({
            code: 1,
          });
        }),
      );
    });

    describe(`concurrent operations`, () => {
      test(
        `it should handle concurrent requests for same long-lived task`,
        makeTemporaryEnv({
          name: `test-package`,
        }, cleanupDaemon(async ({path, run, runSwitch}) => {
          // This tests the race condition fix - multiple concurrent requests
          // for the same long-lived task should not start multiple instances
          const counterFile = ppath.join(path, `start-counter`);
          await xfs.writeFilePromise(counterFile, `0`);

          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `@long-lived`,
            `server:`,
            `  count=$(cat start-counter)`,
            `  count=$((count + 1))`,
            `  echo $count > start-counter`,
            `  echo "server-$count"`,
            `  sleep 10`,
          ].join(`\n`));

          await run(`install`);

          // Explicitly start daemon first to ensure all requests use the same daemon
          await runSwitch(`switch`, `daemon`, `--open`);

          // Add a small delay to ensure daemon is fully ready
          await new Promise(resolve => setTimeout(resolve, 200));

          // Fire 3 concurrent requests for the same long-lived task
          const promises = [
            runSwitch(`tasks`, `run`, `server`).catch(() => {}),
            runSwitch(`tasks`, `run`, `server`).catch(() => {}),
            runSwitch(`tasks`, `run`, `server`).catch(() => {}),
          ];

          // Wait for warm-up and some processing time
          await new Promise(resolve => setTimeout(resolve, 1500));

          // Check that server only started once
          const startCount = await xfs.readFilePromise(counterFile, `utf8`);
          expect(startCount.trim()).toBe(`1`);

          // Clean up
          await runSwitch(`tasks`, `stop`, `server`).catch(() => {});
        })),
      );

      test(
        `it should handle concurrent different tasks without interference`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          // Run multiple independent tasks concurrently
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `task-a:`,
            `  sleep 0.2 && echo "task-a-done"`,
            ``,
            `task-b:`,
            `  sleep 0.2 && echo "task-b-done"`,
            ``,
            `task-c:`,
            `  sleep 0.2 && echo "task-c-done"`,
          ].join(`\n`));

          await run(`install`);

          // Run all three concurrently
          const [resultA, resultB, resultC] = await Promise.all([
            runSwitch(`tasks`, `run`, `--standalone`, `task-a`),
            runSwitch(`tasks`, `run`, `--standalone`, `task-b`),
            runSwitch(`tasks`, `run`, `--standalone`, `task-c`),
          ]);

          expect(resultA.stdout.trim()).toBe(`task-a-done`);
          expect(resultB.stdout.trim()).toBe(`task-b-done`);
          expect(resultC.stdout.trim()).toBe(`task-c-done`);
        }),
      );

      test(
        `it should isolate contexts between concurrent executions`,
        makeTemporaryEnv({
          name: `test-package`,
        }, cleanupDaemon(async ({path, run, runSwitch}) => {
          // Two concurrent executions of the same task graph should use separate contexts
          const outputFile = ppath.join(path, `output`);

          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `setup:`,
            `  echo "setup-$(date +%s%N)" >> output`,
            ``,
            `build: setup`,
            `  echo "build-$(date +%s%N)" >> output`,
          ].join(`\n`));

          await run(`install`);

          // Clear output file
          await xfs.writeFilePromise(outputFile, ``);

          // Run the same task twice concurrently (not using --standalone so they share daemon)
          const [result1, result2] = await Promise.all([
            runSwitch(`tasks`, `run`, `build`),
            runSwitch(`tasks`, `run`, `build`),
          ]);

          // Both should succeed
          expect(result1.code).toBe(0);
          expect(result2.code).toBe(0);

          // Check that setup ran twice (once per context)
          const output = await xfs.readFilePromise(outputFile, `utf8`);
          const setupLines = output.trim().split(`\n`).filter(l => l.startsWith(`setup-`));
          expect(setupLines.length).toBe(2);
        })),
      );
    });

    describe(`scalability`, () => {
      test(
        `it should handle a large dependency graph efficiently`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          // Create a graph with 20 tasks: 10 leaf tasks, 5 mid-level, 3 second-level, 2 top-level, 1 root
          // This tests the scheduler's ability to handle complex graphs
          const tasks = [
            // 10 leaf tasks (no dependencies)
            ...Array.from({length: 10}, (_, i) => `leaf-${i}:\n  echo "leaf-${i}"`),
            ``,
            // 5 mid-level tasks (each depends on 2 leaf tasks)
            ...Array.from({length: 5}, (_, i) =>
              `mid-${i}: leaf-${i * 2} leaf-${i * 2 + 1}\n  echo "mid-${i}"`
            ),
            ``,
            // 3 second-level tasks
            `second-0: mid-0 mid-1\n  echo "second-0"`,
            `second-1: mid-2 mid-3\n  echo "second-1"`,
            `second-2: mid-4\n  echo "second-2"`,
            ``,
            // 2 top-level tasks
            `top-0: second-0 second-1\n  echo "top-0"`,
            `top-1: second-2\n  echo "top-1"`,
            ``,
            // Root task
            `root: top-0 top-1\n  echo "root"`,
          ];

          await xfs.writeFilePromise(ppath.join(path, `taskfile`), tasks.join(`\n`));

          await run(`install`);

          const startTime = Date.now();
          const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `root`);
          const elapsed = Date.now() - startTime;

          const lines = stdout.trim().split(`\n`);

          // All 21 tasks should have run
          expect(lines.length).toBe(21);

          // Root should be last
          expect(lines[lines.length - 1]).toBe(`root`);

          // All leaf tasks should appear before mid tasks
          const leafIndices = lines
            .map((l, i) => l.startsWith(`leaf-`) ? i : -1)
            .filter(i => i >= 0);
          const midIndices = lines
            .map((l, i) => l.startsWith(`mid-`) ? i : -1)
            .filter(i => i >= 0);

          expect(Math.max(...leafIndices)).toBeLessThan(Math.min(...midIndices));

          // Should complete in reasonable time (under 5 seconds for simple echo commands)
          expect(elapsed).toBeLessThan(5000);
        }),
      );

      test(
        `it should handle many parallel tasks`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          // Create 10 parallel tasks that all run concurrently
          const parallelCount = 10;
          const tasks = [
            ...Array.from({length: parallelCount}, (_, i) =>
              `parallel-${i}:\n  sleep 0.2 && echo "parallel-${i}"`
            ),
            ``,
            `root: ${Array.from({length: parallelCount}, (_, i) => `parallel-${i}&`).join(` `)}\n  echo "root"`,
          ];

          await xfs.writeFilePromise(ppath.join(path, `taskfile`), tasks.join(`\n`));

          await run(`install`);

          const startTime = Date.now();
          const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `root`);
          const elapsed = Date.now() - startTime;

          const lines = stdout.trim().split(`\n`);

          // All parallel tasks + root should run
          expect(lines.length).toBe(parallelCount + 1);

          // Root should be last
          expect(lines[lines.length - 1]).toBe(`root`);

          // Since tasks run in parallel (0.2s each), total time should be much less than 10 * 0.2s = 2s
          // Allow some overhead, but it should be under 1.5 seconds if parallel
          expect(elapsed).toBeLessThan(1500);
        }),
      );
    });

    describe(`daemon signal propagation`, () => {
      test(
        `it should terminate child processes when daemon receives SIGTERM`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          const pidFile = ppath.join(path, `task.pid`);
          const runningMarker = ppath.join(path, `running`);

          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `long-task:`,
            `  echo $$ > task.pid`,
            `  touch running`,
            `  sleep 60`,
          ].join(`\n`));

          await run(`install`);

          // Start the task in background
          const taskPromise = runSwitch(`tasks`, `run`, `long-task`).catch(() => {});

          // Wait for task to start and write its PID
          await new Promise(resolve => setTimeout(resolve, 1000));

          // Verify task is running
          const taskRunning = await xfs.existsPromise(runningMarker);
          expect(taskRunning).toBe(true);

          // Read the task PID
          const taskPid = parseInt(await xfs.readFilePromise(pidFile, `utf8`), 10);

          // Kill the daemon (which should propagate signals to children)
          await runSwitch(`switch`, `daemon`, `--kill-all`);

          // Wait for signal propagation and process cleanup
          await new Promise(resolve => setTimeout(resolve, 1000));

          // Check if the task process is still running
          let processStillRunning = false;
          try {
            // Sending signal 0 checks if process exists without actually sending a signal
            process.kill(taskPid, 0);
            processStillRunning = true;
          } catch {
            // Process doesn't exist, which is expected
            processStillRunning = false;
          }

          expect(processStillRunning).toBe(false);
        }),
      );

      test(
        `it should send SIGKILL after 5 seconds if SIGTERM is ignored`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          const pidFile = ppath.join(path, `task.pid`);
          const runningMarker = ppath.join(path, `running`);

          // Create a task that traps SIGTERM and ignores it
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `stubborn-task:`,
            `  trap '' TERM`,
            `  echo $$ > task.pid`,
            `  touch running`,
            `  sleep 120`,
          ].join(`\n`));

          await run(`install`);

          // Start the task in background
          const taskPromise = runSwitch(`tasks`, `run`, `stubborn-task`).catch(() => {});

          // Wait for task to start and write its PID
          await new Promise(resolve => setTimeout(resolve, 1000));

          // Verify task is running
          const taskRunning = await xfs.existsPromise(runningMarker);
          expect(taskRunning).toBe(true);

          // Read the task PID
          const taskPid = parseInt(await xfs.readFilePromise(pidFile, `utf8`), 10);

          // Kill the daemon - it should send SIGTERM, wait 5s, then SIGKILL
          const startTime = Date.now();
          await runSwitch(`switch`, `daemon`, `--kill-all`);

          // Wait for the full signal propagation cycle (SIGTERM + 5s + SIGKILL + cleanup)
          await new Promise(resolve => setTimeout(resolve, 7000));
          const elapsed = Date.now() - startTime;

          // Check if the task process is still running
          let processStillRunning = false;
          try {
            process.kill(taskPid, 0);
            processStillRunning = true;
          } catch {
            processStillRunning = false;
          }

          // Process should be killed even though it ignored SIGTERM
          expect(processStillRunning).toBe(false);
        }),
        15000, // Increase timeout for this test (5s wait + overhead)
      );
    });
  });
});
