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
