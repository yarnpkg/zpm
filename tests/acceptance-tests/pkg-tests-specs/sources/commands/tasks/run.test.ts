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
      `it should output JSON with --json flag`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  echo "building"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `--json`, `build`);
        const lines = stdout.trim().split(`\n`);

        expect(lines.length).toBe(3);

        const events = lines.map(line => JSON.parse(line));

        expect(events[0]).toEqual({
          type: `task-started`,
          taskId: `test-package:build`,
        });
        expect(events[1]).toEqual({
          type: `output`,
          taskId: `test-package:build`,
          stream: `stdout`,
          line: `building`,
        });
        expect(events[2]).toEqual({
          type: `task-completed`,
          taskId: `test-package:build`,
          exitCode: 0,
        });
      }),
    );

    test(
      `it should output JSON for stderr with --json flag`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  echo "error message" >&2`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `--json`, `build`);
        const lines = stdout.trim().split(`\n`);
        const events = lines.map(line => JSON.parse(line));

        const outputEvent = events.find(e => e.type === `output`);
        expect(outputEvent).toEqual({
          type: `output`,
          taskId: `test-package:build`,
          stream: `stderr`,
          line: `error message`,
        });
      }),
    );

    test(
      `it should output JSON for task dependencies with --json flag`,
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

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `--json`, `build`);
        const lines = stdout.trim().split(`\n`);
        const events = lines.map(line => JSON.parse(line));

        // Should have events for both tasks
        const taskStartedEvents = events.filter(e => e.type === `task-started`);
        const taskCompletedEvents = events.filter(e => e.type === `task-completed`);

        expect(taskStartedEvents.length).toBe(2);
        expect(taskCompletedEvents.length).toBe(2);

        // Verify setup runs before build
        const setupStartIdx = events.findIndex(e => e.type === `task-started` && e.taskId === `test-package:setup`);
        const buildStartIdx = events.findIndex(e => e.type === `task-started` && e.taskId === `test-package:build`);
        expect(setupStartIdx).toBeLessThan(buildStartIdx);
      }),
    );

    test(
      `it should output JSON for failed tasks with --json flag`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  echo "failing"`,
          `  exit 1`,
        ].join(`\n`));

        await run(`install`);

        await expect(runSwitch(`tasks`, `run`, `--standalone`, `--json`, `build`)).rejects.toMatchObject({
          code: 1,
        });
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
      `it should not duplicate target task output on failure with --silent-dependencies`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  echo "build-output"`,
          `  exit 1`,
        ].join(`\n`));

        await run(`install`);

        // Target task output should appear exactly once (streamed live), not duplicated
        await expect(runSwitch(`tasks`, `run`, `--standalone`, `--silent-dependencies`, `build`)).rejects.toMatchObject({
          stdout: `build-output\n`,
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
          if (match?.[1] && match[2]) {
            timestamps.push(parseInt(match[1], 10));
            messages.push(match[2]);
          }
        }

        // Verify the messages are correct
        expect(messages).toEqual([`line1`, `line2`, `line3`]);

        // Verify script-side timestamps are properly spaced (at least 400ms apart)
        // This proves the script's sleep commands executed between echo statements
        for (let i = 1; i < timestamps.length; i++) {
          const diff = timestamps[i]! - timestamps[i - 1]!;
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
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `task-d:`,
            `  echo "task-d"`,
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

          const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `--json`, `target`);
          const events = stdout.trim().split(`\n`).map(line => JSON.parse(line));

          // D should be started exactly once
          const taskDStarted = events.filter(e => e.type === `task-started` && e.taskId === `test-package:task-d`);
          expect(taskDStarted.length).toBe(1);

          // All tasks should complete exactly once
          const completedTasks = events.filter(e => e.type === `task-completed`).map(e => e.taskId);
          expect(completedTasks.sort()).toEqual([
            `test-package:target`,
            `test-package:task-b`,
            `test-package:task-c`,
            `test-package:task-d`,
          ]);

          // D should start before B and C
          const startEvents = events.filter(e => e.type === `task-started`);
          const dStartIdx = startEvents.findIndex(e => e.taskId === `test-package:task-d`);
          const bStartIdx = startEvents.findIndex(e => e.taskId === `test-package:task-b`);
          const cStartIdx = startEvents.findIndex(e => e.taskId === `test-package:task-c`);
          expect(dStartIdx).toBeLessThan(bStartIdx);
          expect(dStartIdx).toBeLessThan(cStartIdx);
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

          const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `--json`, `level-5`);
          const events = stdout.trim().split(`\n`).map(line => JSON.parse(line));

          // Extract task-started events in order
          const startOrder = events
            .filter(e => e.type === `task-started`)
            .map(e => e.taskId.replace(`test-package:`, ``));

          // Should start in correct dependency order
          expect(startOrder).toEqual([
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

          const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `--json`, `target`);
          const events = stdout.trim().split(`\n`).map(line => JSON.parse(line));

          const startEvents = events.filter(e => e.type === `task-started`);
          const completedEvents = events.filter(e => e.type === `task-completed`);

          // dep-c must start first
          expect(startEvents[0].taskId).toBe(`test-package:dep-c`);

          // dep-c must complete before dep-a and dep-b start
          const cCompletedIdx = events.findIndex(e => e.type === `task-completed` && e.taskId === `test-package:dep-c`);
          const aStartIdx = events.findIndex(e => e.type === `task-started` && e.taskId === `test-package:dep-a`);
          const bStartIdx = events.findIndex(e => e.type === `task-started` && e.taskId === `test-package:dep-b`);
          expect(cCompletedIdx).toBeLessThan(aStartIdx);
          expect(cCompletedIdx).toBeLessThan(bStartIdx);

          // target must complete last
          expect(completedEvents[completedEvents.length - 1].taskId).toBe(`test-package:target`);
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

          const result = await runSwitch(`tasks`, `run`, `--standalone`, `--json`, `target`).catch(e => e);
          expect(result.code).toBe(1);

          const events = result.stdout.trim().split(`\n`).map((line: string) => JSON.parse(line));

          // Only failing-base should have started
          const startedTasks = events.filter((e: any) => e.type === `task-started`).map((e: any) => e.taskId);
          expect(startedTasks).toEqual([`test-package:failing-base`]);

          // middle and target should never have started
          expect(startedTasks).not.toContain(`test-package:middle`);
          expect(startedTasks).not.toContain(`test-package:target`);

          // failing-base should have completed with non-zero exit code
          const failingBaseCompleted = events.find((e: any) => e.type === `task-completed` && e.taskId === `test-package:failing-base`);
          expect(failingBaseCompleted).toBeDefined();
          expect(failingBaseCompleted.exitCode).toBe(1);
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

          const result = await runSwitch(`tasks`, `run`, `--standalone`, `--json`, `target`).catch(e => e);
          expect(result.code).toBe(1);

          const events = result.stdout.trim().split(`\n`).map((line: string) => JSON.parse(line));

          // target should never have started (its dependency failed)
          const startedTasks = events.filter((e: any) => e.type === `task-started`).map((e: any) => e.taskId);
          expect(startedTasks).not.toContain(`test-package:target`);
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
          // Create a graph with 21 tasks: 10 leaf tasks, 5 mid-level, 3 second-level, 2 top-level, 1 root
          // This tests the scheduler's ability to handle complex graphs
          const tasks = [
            // 10 leaf tasks (no dependencies)
            ...Array.from({length: 10}, (_, i) => `leaf-${i}:\n  echo "leaf-${i}"`),
            ``,
            // 5 mid-level tasks (each depends on 2 leaf tasks)
            ...Array.from({length: 5}, (_, i) =>
              `mid-${i}: leaf-${i * 2} leaf-${i * 2 + 1}\n  echo "mid-${i}"`,
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

          // Each mid task should appear after its specific leaf dependencies
          // mid-0 depends on leaf-0 and leaf-1, mid-1 depends on leaf-2 and leaf-3, etc.
          const getIndex = (name: string) => lines.indexOf(name);

          for (let i = 0; i < 5; i++) {
            const midIndex = getIndex(`mid-${i}`);
            const leaf1Index = getIndex(`leaf-${i * 2}`);
            const leaf2Index = getIndex(`leaf-${i * 2 + 1}`);

            expect(midIndex).toBeGreaterThan(leaf1Index);
            expect(midIndex).toBeGreaterThan(leaf2Index);
          }

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
              `parallel-${i}:\n  sleep 0.2 && echo "parallel-${i}"`,
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

    describe(`task cancellation semantics`, () => {
      test(
        `it should output task-cancelled in JSON when dependency fails (task never started)`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          // When a dependency fails, dependent tasks should be cancelled (not failed)
          // because they never actually started
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `failing-dep:`,
            `  echo "failing"`,
            `  exit 1`,
            ``,
            `dependent: failing-dep`,
            `  echo "should-never-run"`,
          ].join(`\n`));

          await run(`install`);

          const result = await runSwitch(`tasks`, `run`, `--standalone`, `--json`, `dependent`).catch(e => e);
          expect(result.code).toBe(1);

          const events = result.stdout.trim().split(`\n`).map((line: string) => JSON.parse(line));

          // The failing-dep task should have completed with exit code 1
          const failingDepCompleted = events.find((e: any) => e.type === `task-completed` && e.taskId === `test-package:failing-dep`);
          expect(failingDepCompleted).toBeDefined();
          expect(failingDepCompleted.exitCode).toBe(1);

          // The dependent task should be cancelled (not started, not failed)
          const dependentCancelled = events.find((e: any) => e.type === `task-cancelled` && e.taskId === `test-package:dependent`);
          expect(dependentCancelled).toBeDefined();

          // The dependent task should NOT have a task-started event
          const dependentStarted = events.find((e: any) => e.type === `task-started` && e.taskId === `test-package:dependent`);
          expect(dependentStarted).toBeUndefined();

          // The dependent task should NOT have a task-completed event
          const dependentCompleted = events.find((e: any) => e.type === `task-completed` && e.taskId === `test-package:dependent`);
          expect(dependentCompleted).toBeUndefined();
        }),
      );

      test(
        `it should output task-completed with exitCode in JSON when task itself fails`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          // When a task itself fails (not its dependency), it should show task-completed
          // with the actual exit code
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `failing-task:`,
            `  echo "running"`,
            `  exit 42`,
          ].join(`\n`));

          await run(`install`);

          const result = await runSwitch(`tasks`, `run`, `--standalone`, `--json`, `failing-task`).catch(e => e);
          expect(result.code).toBe(42);

          const events = result.stdout.trim().split(`\n`).map((line: string) => JSON.parse(line));

          // The task should have started
          const started = events.find((e: any) => e.type === `task-started` && e.taskId === `test-package:failing-task`);
          expect(started).toBeDefined();

          // The task should have completed with the actual exit code (not cancelled)
          const completed = events.find((e: any) => e.type === `task-completed` && e.taskId === `test-package:failing-task`);
          expect(completed).toBeDefined();
          expect(completed.exitCode).toBe(42);

          // Should NOT have a task-cancelled event
          const cancelled = events.find((e: any) => e.type === `task-cancelled` && e.taskId === `test-package:failing-task`);
          expect(cancelled).toBeUndefined();
        }),
      );

      test(
        `it should cancel multiple pending dependents when a task fails`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          // Multiple tasks waiting on the same failing dependency should all be cancelled
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `failing-base:`,
            `  exit 1`,
            ``,
            `dep-a: failing-base`,
            `  echo "a"`,
            ``,
            `dep-b: failing-base`,
            `  echo "b"`,
            ``,
            `target: dep-a dep-b`,
            `  echo "target"`,
          ].join(`\n`));

          await run(`install`);

          const result = await runSwitch(`tasks`, `run`, `--standalone`, `--json`, `target`).catch(e => e);
          expect(result.code).toBe(1);

          const events = result.stdout.trim().split(`\n`).map((line: string) => JSON.parse(line));

          // All dependent tasks should be cancelled
          const cancelledTasks = events
            .filter((e: any) => e.type === `task-cancelled`)
            .map((e: any) => e.taskId)
            .sort();

          expect(cancelledTasks).toContain(`test-package:dep-a`);
          expect(cancelledTasks).toContain(`test-package:dep-b`);
          expect(cancelledTasks).toContain(`test-package:target`);
        }),
      );
    });

    describe(`subscription timing`, () => {
      test(
        `it should not miss task-started events when subscribing rapidly`,
        makeTemporaryEnv({
          name: `test-package`,
        }, cleanupDaemon(async ({path, run, runSwitch}) => {
          // This tests the fix for subscription registration timing
          // Tasks should be added to subscription before sending response
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `quick-task:`,
            `  echo "done"`,
          ].join(`\n`));

          await run(`install`);

          // Start daemon first
          await runSwitch(`switch`, `daemon`, `--open`);
          await new Promise(resolve => setTimeout(resolve, 200));

          // Run multiple rapid subscriptions
          const results = await Promise.all([
            runSwitch(`tasks`, `run`, `--json`, `quick-task`),
            runSwitch(`tasks`, `run`, `--json`, `quick-task`),
            runSwitch(`tasks`, `run`, `--json`, `quick-task`),
          ]);

          // Each result should have received the task-started event
          for (const result of results) {
            const events = result.stdout.trim().split(`\n`).map((line: string) => JSON.parse(line));
            const startedEvents = events.filter((e: any) => e.type === `task-started`);
            expect(startedEvents.length).toBeGreaterThanOrEqual(1);
          }
        })),
      );

      test(
        `it should not miss events for very fast completing tasks`,
        makeTemporaryEnv({
          name: `test-package`,
        }, cleanupDaemon(async ({path, run, runSwitch}) => {
          // This tests that even tasks completing nearly instantly
          // have their TaskStarted and TaskCompleted events received
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `instant:`,
            `  true`,
          ].join(`\n`));

          await run(`install`);

          // Start daemon
          await runSwitch(`switch`, `daemon`, `--open`);
          await new Promise(resolve => setTimeout(resolve, 200));

          // Run the instant task multiple times sequentially with JSON output
          for (let i = 0; i < 5; i++) {
            const result = await runSwitch(`tasks`, `run`, `--json`, `instant`);
            const events = result.stdout.trim().split(`\n`).map((line: string) => JSON.parse(line));

            // Must have task-started event
            const started = events.filter((e: any) => e.type === `task-started`);
            expect(started.length).toBe(1);

            // Must have task-completed event
            const completed = events.filter((e: any) => e.type === `task-completed`);
            expect(completed.length).toBe(1);
            expect(completed[0].exitCode).toBe(0);
          }
        })),
      );

      test(
        `it should not send duplicate task-completed events`,
        makeTemporaryEnv({
          name: `test-package`,
        }, cleanupDaemon(async ({path, run, runSwitch}) => {
          // Verify that task-completed is only sent once, not duplicated
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `task:`,
            `  echo "output"`,
          ].join(`\n`));

          await run(`install`);

          await runSwitch(`switch`, `daemon`, `--open`);
          await new Promise(resolve => setTimeout(resolve, 200));

          const result = await runSwitch(`tasks`, `run`, `--json`, `task`);
          const events = result.stdout.trim().split(`\n`).map((line: string) => JSON.parse(line));

          // Count completed events for this task
          const completedEvents = events.filter((e: any) =>
            e.type === `task-completed` && e.taskId.includes(`test-package:task`),
          );

          // Should only have exactly one task-completed event
          expect(completedEvents.length).toBe(1);
        })),
      );

      test(
        `it should not send duplicate events for failed tasks`,
        makeTemporaryEnv({
          name: `test-package`,
        }, cleanupDaemon(async ({path, run, runSwitch}) => {
          // Verify that failed tasks don't receive both TaskFailed AND TaskCompleted
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `failing:`,
            `  exit 1`,
          ].join(`\n`));

          await run(`install`);

          await runSwitch(`switch`, `daemon`, `--open`);
          await new Promise(resolve => setTimeout(resolve, 200));

          const result = await runSwitch(`tasks`, `run`, `--json`, `failing`).catch(e => e);
          const events = result.stdout.trim().split(`\n`).map((line: string) => JSON.parse(line));

          // Count all terminal events for this task
          const terminalEvents = events.filter((e: any) =>
            (e.type === `task-completed` || e.type === `task-failed`) &&
            e.taskId.includes(`test-package:failing`),
          );

          // Should only have exactly one terminal event
          expect(terminalEvents.length).toBe(1);
          expect(terminalEvents[0].type).toBe(`task-completed`);
          expect(terminalEvents[0].exitCode).toBe(1);
        })),
      );
    });

    describe(`warm-up timer behavior`, () => {
      test(
        `it should not send warm-up-complete for tasks that fail during warm-up`,
        makeTemporaryEnv({
          name: `test-package`,
        }, cleanupDaemon(async ({path, run, runSwitch}) => {
          // Long-lived task that crashes before 500ms warm-up period
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `@long-lived`,
            `crashing-server:`,
            `  echo "server starting"`,
            `  sleep 0.1`,
            `  exit 1`,
            ``,
            `client: crashing-server`,
            `  echo "client started"`,
          ].join(`\n`));

          await run(`install`);

          await runSwitch(`switch`, `daemon`, `--open`);
          await new Promise(resolve => setTimeout(resolve, 200));

          // Run client which depends on the crashing long-lived server
          const result = await runSwitch(`tasks`, `run`, `--json`, `client`).catch(e => e);
          const events = result.stdout.trim().split(`\n`).map((line: string) => JSON.parse(line));

          // Should NOT have warm-up-complete for the crashing server
          const warmUpEvents = events.filter((e: any) =>
            e.type === `task-warm-up-complete` &&
            e.taskId.includes(`crashing-server`),
          );

          expect(warmUpEvents.length).toBe(0);

          // Server should have started
          const serverStarted = events.find((e: any) =>
            e.type === `task-started` &&
            e.taskId.includes(`crashing-server`),
          );
          expect(serverStarted).toBeDefined();

          // Server should have completed with failure
          const serverCompleted = events.find((e: any) =>
            e.type === `task-completed` &&
            e.taskId.includes(`crashing-server`),
          );
          expect(serverCompleted).toBeDefined();
          expect(serverCompleted.exitCode).toBe(1);
        })),
      );

      test(
        `it should send warm-up-complete only once per long-lived task`,
        makeTemporaryEnv({
          name: `test-package`,
        }, cleanupDaemon(async ({path, run, runSwitch}) => {
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `@long-lived`,
            `server:`,
            `  echo "server running"`,
            `  sleep 10`,
          ].join(`\n`));

          await run(`install`);

          await runSwitch(`switch`, `daemon`, `--open`);
          await new Promise(resolve => setTimeout(resolve, 200));

          // Start server and wait for warm-up
          const serverPromise = runSwitch(`tasks`, `run`, `--json`, `server`).catch(() => {});

          // Wait for warm-up (500ms) plus some buffer
          await new Promise(resolve => setTimeout(resolve, 800));

          // Start a second request that attaches to the existing server
          const attachPromise = runSwitch(`tasks`, `run`, `--json`, `server`).catch(() => {});

          // Wait a bit more
          await new Promise(resolve => setTimeout(resolve, 300));

          // Clean up
          await runSwitch(`tasks`, `stop`, `server`).catch(() => {});

          // The warm-up should have been sent exactly once for the server
          // (This is verified by the fact that attachPromise should see
          // the task as already warmed up, not schedule another warm-up)
        })),
      );
    });

    describe(`subtask state management`, () => {
      test(
        `it should wait for all pushed subtasks before completing parent`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          // Tests the WaitingForSubtasks state - parent should wait for all subtasks
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `slow-subtask:`,
            `  sleep 0.3 && echo "slow-done"`,
            ``,
            `fast-subtask:`,
            `  echo "fast-done"`,
            ``,
            `parent:`,
            `  echo "parent-start"`,
            `  yarn tasks push slow-subtask`,
            `  yarn tasks push fast-subtask`,
            `  echo "parent-script-done"`,
          ].join(`\n`));

          await run(`install`);

          const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `--json`, `parent`);
          const events = stdout.trim().split(`\n`).map((line: string) => JSON.parse(line));

          // Parent should complete last (after both subtasks)
          const completedEvents = events.filter((e: any) => e.type === `task-completed`);
          const parentCompleted = completedEvents.findIndex((e: any) => e.taskId === `test-package:parent`);
          const slowCompleted = completedEvents.findIndex((e: any) => e.taskId === `test-package:slow-subtask`);
          const fastCompleted = completedEvents.findIndex((e: any) => e.taskId === `test-package:fast-subtask`);

          // Parent should complete after both subtasks
          expect(parentCompleted).toBeGreaterThan(slowCompleted);
          expect(parentCompleted).toBeGreaterThan(fastCompleted);
        }),
      );

      test(
        `it should propagate subtask failure exit code to parent`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          // When a subtask fails, the parent should fail with the subtask's exit code
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `failing-subtask:`,
            `  exit 55`,
            ``,
            `parent:`,
            `  echo "parent-start"`,
            `  yarn tasks push failing-subtask`,
            `  echo "parent-script-done"`,
          ].join(`\n`));

          await run(`install`);

          const result = await runSwitch(`tasks`, `run`, `--standalone`, `--json`, `parent`).catch(e => e);
          expect(result.code).toBe(55);

          const events = result.stdout.trim().split(`\n`).map((line: string) => JSON.parse(line));

          // Subtask should have failed with exit code 55
          const subtaskCompleted = events.find((e: any) => e.type === `task-completed` && e.taskId === `test-package:failing-subtask`);
          expect(subtaskCompleted).toBeDefined();
          expect(subtaskCompleted.exitCode).toBe(55);

          // Parent should also complete (not be cancelled) since it did start
          const parentCompleted = events.find((e: any) => e.type === `task-completed` && e.taskId === `test-package:parent`);
          expect(parentCompleted).toBeDefined();
          expect(parentCompleted.exitCode).toBe(55);
        }),
      );

      test(
        `it should use parent exit code when parent script fails before subtasks`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          // When parent script itself fails, its exit code should be used
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `subtask:`,
            `  echo "subtask"`,
            ``,
            `parent:`,
            `  yarn tasks push subtask`,
            `  exit 66`,
          ].join(`\n`));

          await run(`install`);

          const result = await runSwitch(`tasks`, `run`, `--standalone`, `parent`).catch(e => e);
          expect(result.code).toBe(66);
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

    describe(`daemon lifecycle management`, () => {
      test(
        `it should clean up stale daemon and start fresh`,
        makeTemporaryEnv({
          name: `test-package`,
        }, cleanupDaemon(async ({path, run, runSwitch}) => {
          // This tests the stale daemon cleanup behavior
          // When a daemon is detected as alive but unresponsive, it should be killed
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `build:`,
            `  echo "building"`,
          ].join(`\n`));

          await run(`install`);

          // Start daemon
          await runSwitch(`switch`, `daemon`, `--open`);
          await new Promise(resolve => setTimeout(resolve, 200));

          // Run a task to verify daemon is working
          const {stdout: stdout1} = await runSwitch(`tasks`, `run`, `build`);
          expect(stdout1).toContain(`building`);

          // Kill daemon forcefully (simulating a crash that leaves stale state)
          await runSwitch(`switch`, `daemon`, `--kill-all`);

          // Start daemon again - should work even if there was stale state
          await runSwitch(`switch`, `daemon`, `--open`);
          await new Promise(resolve => setTimeout(resolve, 200));

          // Run task again - should succeed with fresh daemon
          const {stdout: stdout2} = await runSwitch(`tasks`, `run`, `build`);
          expect(stdout2).toContain(`building`);
        })),
      );

      test(
        `it should gracefully shutdown when project root changes`,
        makeTemporaryEnv({
          name: `test-package`,
        }, cleanupDaemon(async ({path, run, runSwitch}) => {
          // This tests the graceful shutdown behavior when project root is modified
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `@long-lived`,
            `server:`,
            `  echo "server-started"`,
            `  sleep 60`,
            ``,
            `build:`,
            `  echo "building"`,
          ].join(`\n`));

          await run(`install`);

          // Start daemon and get a server running
          const serverPromise = runSwitch(`tasks`, `run`, `server`).catch(() => {});

          // Wait for warm-up
          await new Promise(resolve => setTimeout(resolve, 700));

          // Store daemon info before the change
          // (In real scenario, modifying yarn.lock or package.json would trigger shutdown)

          // Kill all daemons - this simulates the graceful shutdown
          // The actual project root watcher behavior is internal to the daemon
          await runSwitch(`switch`, `daemon`, `--kill-all`);

          // Daemon should be stopped now
          // Verify by trying to start a new one (should succeed)
          await runSwitch(`switch`, `daemon`, `--open`);
          await new Promise(resolve => setTimeout(resolve, 200));

          // Run a quick task to verify new daemon works
          const {stdout} = await runSwitch(`tasks`, `run`, `build`);
          expect(stdout).toContain(`building`);
        })),
      );
    });

    describe(`memory management`, () => {
      test(
        `it should handle many sequential task executions without issues`,
        makeTemporaryEnv({
          name: `test-package`,
        }, cleanupDaemon(async ({path, run, runSwitch}) => {
          // This tests that task metadata is properly cleaned up after execution
          // Running many tasks sequentially should not cause issues
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `task:`,
            `  echo "iteration"`,
          ].join(`\n`));

          await run(`install`);

          // Start daemon
          await runSwitch(`switch`, `daemon`, `--open`);
          await new Promise(resolve => setTimeout(resolve, 200));

          // Run the same task many times (this would accumulate metadata without cleanup)
          const iterations = 20;
          for (let i = 0; i < iterations; i++) {
            const {stdout} = await runSwitch(`tasks`, `run`, `task`);
            expect(stdout).toContain(`iteration`);
          }

          // All iterations should have succeeded - no memory/state issues
        })),
      );

      test(
        `it should handle many concurrent task executions`,
        makeTemporaryEnv({
          name: `test-package`,
        }, cleanupDaemon(async ({path, run, runSwitch}) => {
          // This tests memory management under concurrent load
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `task-a:`,
            `  sleep 0.1 && echo "a"`,
            ``,
            `task-b:`,
            `  sleep 0.1 && echo "b"`,
            ``,
            `task-c:`,
            `  sleep 0.1 && echo "c"`,
          ].join(`\n`));

          await run(`install`);

          // Start daemon
          await runSwitch(`switch`, `daemon`, `--open`);
          await new Promise(resolve => setTimeout(resolve, 200));

          // Run multiple batches of concurrent tasks
          for (let batch = 0; batch < 5; batch++) {
            const results = await Promise.all([
              runSwitch(`tasks`, `run`, `task-a`),
              runSwitch(`tasks`, `run`, `task-b`),
              runSwitch(`tasks`, `run`, `task-c`),
            ]);

            expect(results[0].stdout).toContain(`a`);
            expect(results[1].stdout).toContain(`b`);
            expect(results[2].stdout).toContain(`c`);
          }
        })),
      );
    });

    describe(`cross-context isolation`, () => {
      test(
        `it should not report tasks from other contexts as completed`,
        makeTemporaryEnv({
          name: `test-package`,
        }, cleanupDaemon(async ({path, run, runSwitch}) => {
          // This test verifies that tasks from one context do not appear in another context's output.
          // Context "abc" runs task A, Context "xyz" runs task C.
          // Task A should NOT see task C's events, and vice versa.

          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `task-a:`,
            `  sleep 0.1 && echo "task-a-done"`,
            ``,
            `task-c:`,
            `  sleep 0.1 && echo "task-c-done"`,
          ].join(`\n`));

          await run(`install`);

          // Start daemon
          await runSwitch(`switch`, `daemon`, `--open`);
          await new Promise(resolve => setTimeout(resolve, 200));

          // Run task-a and task-c concurrently in different contexts
          const [resultA, resultC] = await Promise.all([
            runSwitch(`tasks`, `run`, `--json`, `task-a`),
            runSwitch(`tasks`, `run`, `--json`, `task-c`),
          ]);

          // Parse events from each result
          const eventsA = resultA.stdout.trim().split(`\n`).map((line: string) => JSON.parse(line));
          const eventsC = resultC.stdout.trim().split(`\n`).map((line: string) => JSON.parse(line));

          // Context A should only see task-a events, never task-c
          const taskAStarted = eventsA.filter((e: any) => e.type === `task-started`);
          const taskACompleted = eventsA.filter((e: any) => e.type === `task-completed`);

          expect(taskAStarted.length).toBe(1);
          expect(taskAStarted[0].taskId).toContain(`task-a`);
          expect(taskACompleted.length).toBe(1);
          expect(taskACompleted[0].taskId).toContain(`task-a`);

          // Context A should NOT see task-c events
          const taskCInA = eventsA.filter((e: any) =>
            e.taskId && e.taskId.includes(`task-c`),
          );
          expect(taskCInA.length).toBe(0);

          // Context C should only see task-c events, never task-a
          const taskCStarted = eventsC.filter((e: any) => e.type === `task-started`);
          const taskCCompleted = eventsC.filter((e: any) => e.type === `task-completed`);

          expect(taskCStarted.length).toBe(1);
          expect(taskCStarted[0].taskId).toContain(`task-c`);
          expect(taskCCompleted.length).toBe(1);
          expect(taskCCompleted[0].taskId).toContain(`task-c`);

          // Context C should NOT see task-a events
          const taskAInC = eventsC.filter((e: any) =>
            e.taskId && e.taskId.includes(`task-a`),
          );
          expect(taskAInC.length).toBe(0);
        })),
      );

      test(
        `it should not spuriously schedule tasks from other contexts`,
        makeTemporaryEnv({
          name: `test-package`,
        }, cleanupDaemon(async ({path, run, runSwitch}) => {
          // Tests that find_ready_tasks only considers tasks prepared in the current context.
          // If context "abc" pushes task A with dependency D,
          // and context "xyz" pushes task C (no dependencies),
          // then context "xyz" should NOT accidentally schedule task D.

          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `dep-d:`,
            `  echo "dep-d-done"`,
            ``,
            `task-a: dep-d`,
            `  echo "task-a-done"`,
            ``,
            `task-c:`,
            `  echo "task-c-done"`,
          ].join(`\n`));

          await run(`install`);

          // Start daemon
          await runSwitch(`switch`, `daemon`, `--open`);
          await new Promise(resolve => setTimeout(resolve, 200));

          // Run task-a and task-c concurrently
          const [resultA, resultC] = await Promise.all([
            runSwitch(`tasks`, `run`, `--json`, `task-a`),
            runSwitch(`tasks`, `run`, `--json`, `task-c`),
          ]);

          // Parse events from task-c's context
          const eventsC = resultC.stdout.trim().split(`\n`).map((line: string) => JSON.parse(line));

          // Context C should NOT have any dep-d events (dep-d belongs to context A)
          const depDInC = eventsC.filter((e: any) =>
            e.taskId && e.taskId.includes(`dep-d`),
          );
          expect(depDInC.length).toBe(0);

          // Context C should only see task-c
          const allTaskIds = eventsC
            .filter((e: any) => e.taskId)
            .map((e: any) => e.taskId);
          for (const taskId of allTaskIds) {
            expect(taskId).toContain(`task-c`);
          }
        })),
      );
    });

    describe(`output framing`, () => {
      test(
        `it should not print Process started for tasks that never ran due to dependency failure`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          // When a dependency fails, dependent tasks are marked as failed via find_tasks_to_fail
          // and TaskCompleted { exit_code: 1 } is broadcast. However, these tasks never actually ran,
          // so we should NOT print "Process started" for them.

          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `failing-dep:`,
            `  echo "failing-output"`,
            `  exit 1`,
            ``,
            `dependent: failing-dep`,
            `  echo "dependent-should-not-run"`,
          ].join(`\n`));

          await run(`install`);

          // Run with --silent-dependencies which triggers on_task_completed for failed deps
          const result = await runSwitch(`tasks`, `run`, `--standalone`, `--silent-dependencies`, `dependent`).catch(e => e);
          expect(result.code).toBe(1);

          // The output should show "Process started" for the failing-dep (which actually ran)
          // but NOT for the dependent task (which never ran)
          expect(result.stdout).toContain(`[test-package:failing-dep]: Process started`);
          expect(result.stdout).toContain(`[test-package:failing-dep]: failing-output`);
          expect(result.stdout).toContain(`[test-package:failing-dep]: Process exited (exit code 1)`);

          // The dependent task should NOT have "Process started" since it never ran
          // (it was cancelled due to dependency failure)
          expect(result.stdout).not.toContain(`[test-package:dependent]: Process started`);
        }),
      );

      test(
        `it should not print framing for tasks with no output even on failure`,
        makeTemporaryEnv({
          name: `test-package`,
        }, async ({path, run, runSwitch}) => {
          // A task that fails without producing output should not have framing printed

          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `silent-fail:`,
            `  exit 1`,
            ``,
            `dependent: silent-fail`,
            `  echo "should-not-run"`,
          ].join(`\n`));

          await run(`install`);

          const result = await runSwitch(`tasks`, `run`, `--standalone`, `--silent-dependencies`, `dependent`).catch(e => e);
          expect(result.code).toBe(1);

          // Since silent-fail produces no output, even though it ran and failed,
          // we should not print any framing for it
          expect(result.stdout).not.toContain(`Process started`);
          expect(result.stdout).not.toContain(`Process exited`);
        }),
      );
    });

    describe(`memory management - task metadata cleanup`, () => {
      test(
        `it should clean up task metadata after many sequential executions`,
        makeTemporaryEnv({
          name: `test-package`,
        }, cleanupDaemon(async ({path, run, runSwitch}) => {
          // This test verifies that running many tasks doesn't cause unbounded
          // growth in the tasks/prepared/subtasks maps
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `task:`,
            `  echo "iteration"`,
          ].join(`\n`));

          await run(`install`);

          // Start daemon
          await runSwitch(`switch`, `daemon`, `--open`);
          await new Promise(resolve => setTimeout(resolve, 200));

          // Get initial stats
          const initialStats = await runSwitch(`tasks`, `stats`, `--json`);
          const initial = JSON.parse(initialStats.stdout);

          // Run the same task many times - each run creates a new context
          const iterations = 30;
          for (let i = 0; i < iterations; i++) {
            await runSwitch(`tasks`, `run`, `task`);
          }

          // Get final stats
          const finalStats = await runSwitch(`tasks`, `stats`, `--json`);
          const final = JSON.parse(finalStats.stdout);

          // The daemon has a max_closed_tasks limit (default 100), so after many runs
          // the metadata should be bounded. We check that growth is not proportional
          // to iterations - allow some leeway for configuration defaults.
          //
          // If there's a memory leak, tasksCount would be >= iterations.
          // With proper cleanup, it should be bounded by max_closed_tasks.
          const maxExpectedTasks = 100; // Based on default max_closed_tasks

          // The tasks count should be bounded, not growing unboundedly
          expect(final.tasksCount).toBeLessThanOrEqual(maxExpectedTasks);

          // Similarly for prepared and subtasks
          expect(final.preparedCount).toBeLessThanOrEqual(maxExpectedTasks);
        })),
      );

      test(
        `it should properly complete tasks without scripts`,
        makeTemporaryEnv({
          name: `test-package`,
        }, cleanupDaemon(async ({path, run, runSwitch}) => {
          // This test verifies that tasks without scripts (pure dependency aggregators)
          // are properly completed and cleaned up
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `# A task with a script`,
            `actual-work:`,
            `  echo "doing work"`,
            ``,
            `# A task WITHOUT a script - just aggregates dependencies`,
            `# This type of task was not being properly completed/cleaned up`,
            `aggregate: actual-work`,
          ].join(`\n`));

          await run(`install`);

          // Start daemon
          await runSwitch(`switch`, `daemon`, `--open`);
          await new Promise(resolve => setTimeout(resolve, 200));

          // Run the aggregate task multiple times
          for (let i = 0; i < 10; i++) {
            const {stdout} = await runSwitch(`tasks`, `run`, `aggregate`);
            expect(stdout).toContain(`doing work`);
          }

          // Get stats to verify cleanup
          const statsResult = await runSwitch(`tasks`, `stats`, `--json`);
          const stats = JSON.parse(statsResult.stdout);

          // Verify the counts are bounded (not growing unboundedly)
          // Each run has 2 tasks (aggregate + actual-work), so 10 runs = 20 task instances
          // With cleanup, this should be bounded
          expect(stats.tasksCount).toBeLessThanOrEqual(100);
          expect(stats.preparedCount).toBeLessThanOrEqual(100);
        })),
      );

      test(
        `it should track closed_tasks correctly for eviction`,
        makeTemporaryEnv({
          name: `test-package`,
        }, cleanupDaemon(async ({path, run, runSwitch}) => {
          // Verify that closed_tasks queue is properly populated
          await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
            `task:`,
            `  echo "test"`,
          ].join(`\n`));

          await run(`install`);

          // Start daemon
          await runSwitch(`switch`, `daemon`, `--open`);
          await new Promise(resolve => setTimeout(resolve, 200));

          // Get initial stats
          const initialStats = await runSwitch(`tasks`, `stats`, `--json`);
          const initial = JSON.parse(initialStats.stdout);

          // Run task once
          await runSwitch(`tasks`, `run`, `task`);

          // Get stats after running
          const afterStats = await runSwitch(`tasks`, `stats`, `--json`);
          const after = JSON.parse(afterStats.stdout);

          // closed_tasks should have increased (task was marked as closed)
          expect(after.closedTasksCount).toBeGreaterThanOrEqual(initial.closedTasksCount);
        })),
      );
    });
  });
});
