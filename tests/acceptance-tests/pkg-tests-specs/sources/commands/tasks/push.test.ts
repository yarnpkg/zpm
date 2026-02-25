import {ppath, xfs} from '@yarnpkg/fslib';

describe(`Commands`, () => {
  describe(`tasks push`, () => {
    test(
      `it should fail when not running inside a task context`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  echo "building"`,
        ].join(`\n`));

        await run(`install`);

        await expect(run(`tasks`, `push`, `build`)).rejects.toMatchObject({
          code: 1,
          stdout: expect.stringContaining(`Not running inside a task context`),
        });
      }),
    );

    test(
      `it should allow pushing a task from within a running task`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `setup:`,
          `  echo "setup-done"`,
          ``,
          `trigger:`,
          `  yarn tasks push setup`,
          `  echo "trigger-done"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`tasks`, `run`, `trigger`);
        expect(stdout).toContain(`trigger-done`);
        expect(stdout).toContain(`setup-done`);
      }),
    );

    test(
      `it should allow pushing multiple tasks at once`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `task-a:`,
          `  echo "task-a-done"`,
          ``,
          `task-b:`,
          `  echo "task-b-done"`,
          ``,
          `trigger:`,
          `  yarn tasks push task-a task-b`,
          `  echo "trigger-done"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`tasks`, `run`, `trigger`);
        expect(stdout).toContain(`trigger-done`);
        expect(stdout).toContain(`task-a-done`);
        expect(stdout).toContain(`task-b-done`);
      }),
    );

    test(
      `it should fail when pushing a nonexistent task`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `trigger:`,
          `  set -e`,
          `  yarn tasks push nonexistent`,
          `  echo "should not reach here"`,
        ].join(`\n`));

        await run(`install`);

        await expect(run(`tasks`, `run`, `trigger`)).rejects.toMatchObject({
          code: 1,
        });
      }),
    );

    test(
      `it should wait for pushed tasks to complete before task run exits`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `slow-task:`,
          `  sleep 0.2 && echo "slow-task-done"`,
          ``,
          `trigger:`,
          `  yarn tasks push slow-task`,
          `  echo "trigger-done"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`tasks`, `run`, `trigger`);
        expect(stdout).toContain(`trigger-done`);
        expect(stdout).toContain(`slow-task-done`);
      }),
    );

    test(
      `it should handle pushed tasks with dependencies`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `dep-task:`,
          `  echo "dep-task-done"`,
          ``,
          `main-task: dep-task`,
          `  echo "main-task-done"`,
          ``,
          `trigger:`,
          `  yarn tasks push main-task`,
          `  echo "trigger-done"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`tasks`, `run`, `trigger`);
        expect(stdout).toContain(`trigger-done`);
        expect(stdout).toContain(`dep-task-done`);
        expect(stdout).toContain(`main-task-done`);
      }),
    );

    test(
      `it should fail the task run when a pushed task fails`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `failing-task:`,
          `  echo "about-to-fail"`,
          `  exit 1`,
          ``,
          `trigger:`,
          `  yarn tasks push failing-task`,
          `  sleep 0.1`,
          `  echo "trigger-done"`,
        ].join(`\n`));

        await run(`install`);

        await expect(run(`tasks`, `run`, `trigger`)).rejects.toMatchObject({
          code: 1,
        });
      }),
    );

    test(
      `it should not run the same task twice when pushed multiple times`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `counter:`,
          `  echo "counter-ran"`,
          ``,
          `trigger:`,
          `  yarn tasks push counter`,
          `  yarn tasks push counter`,
          `  echo "trigger-done"`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`tasks`, `run`, `trigger`);
        expect(stdout).toContain(`trigger-done`);
        const matches = stdout.match(/counter-ran/g);
        expect(matches).toHaveLength(1);
      }),
    );
  });
});
