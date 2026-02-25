import {ppath, xfs} from '@yarnpkg/fslib';

describe(`Commands`, () => {
  describe(`tasks run`, () => {
    test(
      `it should run a simple task`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  echo "building"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`tasks`, `run`, `build`);
        expect(stdout).toEqual(`building\n`);
      }),
    );

    test(
      `it should run a task with dependencies in order`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `setup:`,
          `  echo "setup"`,
          ``,
          `build: setup`,
          `  echo "build"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`tasks`, `run`, `build`);
        expect(stdout).toEqual(`setup\nbuild\n`);
      }),
    );

    test(
      `it should show prefixes with verbose level 1`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  echo "building"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`tasks`, `run`, `-v`, `build`);
        expect(stdout).toEqual(`[test-package:build]: building\n`);
      }),
    );

    test(
      `it should show prologue and epilogue with verbose level 2`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  echo "building"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`tasks`, `run`, `-vv`, `build`);
        expect(stdout).toEqual(`[test-package:build]: Process started\n[test-package:build]: building\n[test-package:build]: Process exited (exit code 0)\n`);
      }),
    );

    test(
      `it should hide dependency output with --silent-dependencies`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `setup:`,
          `  echo "setup-output"`,
          ``,
          `build: setup`,
          `  echo "build-output"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`tasks`, `run`, `--silent-dependencies`, `build`);
        expect(stdout).toEqual(`build-output\n`);
      }),
    );

    test(
      `it should show dependency output on failure even with --silent-dependencies`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `setup:`,
          `  echo "setup-failure-output"`,
          `  exit 1`,
          ``,
          `build: setup`,
          `  echo "build-output"`,
        ].join(`\n`));

        await run(`install`);

        await expect(run(`tasks`, `run`, `--silent-dependencies`, `build`)).rejects.toMatchObject({
          stdout: `[test-package:setup]: Process started\n[test-package:setup]: setup-failure-output\n[test-package:setup]: Process exited (exit code 1)\n`,
          code: 1,
        });
      }),
    );

    test(
      `it should forward yarn run to task run with silent dependencies`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `setup:`,
          `  echo "setup-output"`,
          ``,
          `build: setup`,
          `  echo "build-output"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`run`, `build`);
        expect(stdout).toEqual(`build-output\n`);
      }),
    );

    test(
      `it should forward yarn run to task run and show verbose output on failure`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `setup:`,
          `  echo "setup-failure-output"`,
          `  exit 1`,
          ``,
          `build: setup`,
          `  echo "build-output"`,
        ].join(`\n`));

        await run(`install`);

        await expect(run(`run`, `build`)).rejects.toMatchObject({
          stdout: `[test-package:setup]: Process started\n[test-package:setup]: setup-failure-output\n[test-package:setup]: Process exited (exit code 1)\n`,
          code: 1,
        });
      }),
    );

    test(
      `it should pass arguments to the target task`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `greet:`,
          `  echo "Hello $1"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`tasks`, `run`, `greet`, `World`);
        expect(stdout).toEqual(`Hello World\n`);
      }),
    );

    test(
      `it should fail when the task does not exist`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  echo "building"`,
        ].join(`\n`));

        await run(`install`);

        await expect(run(`tasks`, `run`, `nonexistent`)).rejects.toMatchObject({
          code: 1,
        });
      }),
    );

    test(
      `it should fail when there is no taskfile`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await run(`install`);

        await expect(run(`tasks`, `run`, `build`)).rejects.toMatchObject({
          code: 1,
        });
      }),
    );

    test(
      `it should run parallel dependencies concurrently`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
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

        const {stdout} = await run(`tasks`, `run`, `build`);

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
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-a/taskfile` as any), [
          `build:`,
          `  echo "building-pkg-a"`,
        ].join(`\n`));

        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/taskfile` as any), [
          `build: pkg-a:build`,
          `  echo "building-pkg-b"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`tasks`, `run`, `build`, {cwd: ppath.join(path, `packages/pkg-b` as any)});
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
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-a/taskfile` as any), [
          `build:`,
          `  echo "building-pkg-a"`,
        ].join(`\n`));

        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/taskfile` as any), [
          `build: pkg-a:build`,
          `  echo "building-pkg-b"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`tasks`, `run`, `--silent-dependencies`, `build`, {cwd: ppath.join(path, `packages/pkg-b` as any)});
        expect(stdout).toEqual(`building-pkg-b\n`);
      }),
    );

    test(
      `it should hide pushed subtask output with --silent-dependencies`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `subtask:`,
          `  echo "subtask-output"`,
          ``,
          `main:`,
          `  yarn tasks push subtask`,
          `  echo "main-output"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`tasks`, `run`, `--silent-dependencies`, `main`);
        expect(stdout).toEqual(`main-output\n`);
      }),
    );

    test(
      `it should return the exit code of the failed task`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  exit 42`,
        ].join(`\n`));

        await run(`install`);

        await expect(run(`tasks`, `run`, `build`)).rejects.toMatchObject({
          code: 42,
        });
      }),
    );
  });
});
