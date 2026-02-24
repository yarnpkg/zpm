import {ppath, xfs} from '@yarnpkg/fslib';

describe(`Commands`, () => {
  describe(`debug resolve-task`, () => {
    test(
      `it should resolve a simple task with no dependencies`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  echo building`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`debug`, `resolve-task`, `build`);
        const result = JSON.parse(stdout);

        expect(result).toEqual({
          "test-package:build": [],
        });
      }),
    );

    test(
      `it should resolve a task with sequential dependencies`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `lint:`,
          `  echo linting`,
          ``,
          `typecheck:`,
          `  echo typechecking`,
          ``,
          `build: lint typecheck`,
          `  echo building`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`debug`, `resolve-task`, `build`);
        const result = JSON.parse(stdout);

        // Sequential deps: lint must complete before typecheck can start
        expect(result).toEqual({
          "test-package:lint": [],
          "test-package:typecheck": [`test-package:lint`],
          "test-package:build": [`test-package:lint`, `test-package:typecheck`],
        });
      }),
    );

    test(
      `it should resolve a task with parallel dependencies`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `lint:`,
          `  echo linting`,
          ``,
          `typecheck:`,
          `  echo typechecking`,
          ``,
          `build: lint& typecheck&`,
          `  echo building`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`debug`, `resolve-task`, `build`);
        const result = JSON.parse(stdout);

        // Parallel deps: lint& and typecheck& can run in parallel (neither waits for the other)
        expect(result).toEqual({
          "test-package:lint": [],
          "test-package:typecheck": [],
          "test-package:build": [`test-package:lint`, `test-package:typecheck`],
        });
      }),
    );

    test(
      `it should fail when the task does not exist`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `build:`,
          `  echo building`,
        ].join(`\n`));

        await run(`install`);

        await expect(run(`debug`, `resolve-task`, `nonexistent`)).rejects.toMatchObject({
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

        await expect(run(`debug`, `resolve-task`, `build`)).rejects.toMatchObject({
          code: 1,
        });
      }),
    );

    test(
      `it should resolve tasks mixing both parallel and sequential dependencies`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        // e: a b& c& d
        // a is sequential, b& c& are parallel, d is sequential
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `a:`,
          `  echo a`,
          ``,
          `b:`,
          `  echo b`,
          ``,
          `c:`,
          `  echo c`,
          ``,
          `d:`,
          `  echo d`,
          ``,
          `e: a b& c& d`,
          `  echo e`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`debug`, `resolve-task`, `e`);
        const result = JSON.parse(stdout);

        // a is sequential (barrier), b& c& are parallel after a, d is sequential after b&c
        // a: [] (no deps)
        // b: [a] (must wait for a)
        // c: [a] (must wait for a, parallel with b)
        // d: [a, b, c] (must wait for the parallel group)
        // e: [a, b, c, d] (must wait for all)
        expect(result).toEqual({
          "test-package:a": [],
          "test-package:b": [`test-package:a`],
          "test-package:c": [`test-package:a`],
          "test-package:d": [`test-package:a`, `test-package:b`, `test-package:c`],
          "test-package:e": [`test-package:a`, `test-package:b`, `test-package:c`, `test-package:d`],
        });
      }),
    );

    test(
      `it should resolve tasks with dependencies on tasks from other workspaces`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`},
        [`packages/pkg-b`]: {name: `pkg-b`, dependencies: {[`pkg-a`]: `workspace:*`}},
      }, async ({path, run}) => {
        // pkg-a has a build task
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-a/taskfile` as any), [
          `build:`,
          `  echo building pkg-a`,
        ].join(`\n`));

        // pkg-b depends on pkg-a:build (pkg-a is declared as a dependency)
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/taskfile` as any), [
          `build: pkg-a:build`,
          `  echo building pkg-b`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`debug`, `resolve-task`, `build`, {cwd: ppath.join(path, `packages/pkg-b` as any)});
        const result = JSON.parse(stdout);

        expect(result).toEqual({
          "pkg-a:build": [],
          "pkg-b:build": [`pkg-a:build`],
        });
      }),
    );

    test(
      `it should fail when there are circular dependencies`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `a: b`,
          `  echo a`,
          ``,
          `b: c`,
          `  echo b`,
          ``,
          `c: a`,
          `  echo c`,
        ].join(`\n`));

        await run(`install`);

        await expect(run(`debug`, `resolve-task`, `a`)).rejects.toMatchObject({
          code: 1,
        });
      }),
    );

    test(
      `it should resolve tasks mixing both parallel and sequential dependencies from other workspaces`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`},
        [`packages/pkg-b`]: {name: `pkg-b`},
        [`packages/pkg-c`]: {name: `pkg-c`, dependencies: {[`pkg-a`]: `workspace:*`, [`pkg-b`]: `workspace:*`}},
      }, async ({path, run}) => {
        // pkg-a and pkg-b have build tasks
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-a/taskfile` as any), [
          `build:`,
          `  echo building pkg-a`,
        ].join(`\n`));

        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/taskfile` as any), [
          `build:`,
          `  echo building pkg-b`,
        ].join(`\n`));

        // pkg-c depends on pkg-a:build& (parallel) and pkg-b:build (sequential)
        // Both pkg-a and pkg-b are declared as dependencies of pkg-c
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-c/taskfile` as any), [
          `build: pkg-a:build& pkg-b:build`,
          `  echo building pkg-c`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`debug`, `resolve-task`, `build`, {cwd: ppath.join(path, `packages/pkg-c` as any)});
        const result = JSON.parse(stdout);

        // pkg-a:build& is parallel, pkg-b:build is sequential after it
        expect(result).toEqual({
          "pkg-a:build": [],
          "pkg-b:build": [`pkg-a:build`],
          "pkg-c:build": [`pkg-a:build`, `pkg-b:build`],
        });
      }),
    );

    test(
      `it should resolve recursive dependencies across workspaces`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`},
        [`packages/pkg-b`]: {name: `pkg-b`, dependencies: {[`pkg-a`]: `workspace:*`}},
        [`packages/pkg-c`]: {name: `pkg-c`, dependencies: {[`pkg-b`]: `workspace:*`}},
      }, async ({path, run}) => {
        // pkg-a:build has no dependencies
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-a/taskfile` as any), [
          `build:`,
          `  echo building pkg-a`,
        ].join(`\n`));

        // pkg-b:build depends on pkg-a:build (pkg-a is declared as a dependency)
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/taskfile` as any), [
          `build: pkg-a:build`,
          `  echo building pkg-b`,
        ].join(`\n`));

        // pkg-c:build depends on pkg-b:build (which transitively depends on pkg-a:build)
        // pkg-b is declared as a dependency of pkg-c
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-c/taskfile` as any), [
          `build: pkg-b:build`,
          `  echo building pkg-c`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`debug`, `resolve-task`, `build`, {cwd: ppath.join(path, `packages/pkg-c` as any)});
        const result = JSON.parse(stdout);

        // Recursive chain: pkg-c -> pkg-b -> pkg-a
        // pkg-a:build has no prerequisites
        // pkg-b:build must wait for pkg-a:build
        // pkg-c:build must wait for both pkg-a:build and pkg-b:build (transitive closure)
        expect(result).toEqual({
          "pkg-a:build": [],
          "pkg-b:build": [`pkg-a:build`],
          "pkg-c:build": [`pkg-a:build`, `pkg-b:build`],
        });
      }),
    );

    test(
      `it should resolve glob pattern dependencies on all matching workspaces`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`},
        [`packages/pkg-b`]: {name: `pkg-b`},
        [`packages/pkg-c`]: {name: `pkg-c`, dependencies: {[`pkg-a`]: `workspace:*`, [`pkg-b`]: `workspace:*`}},
      }, async ({path, run}) => {
        // pkg-a and pkg-b have build tasks
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-a/taskfile` as any), [
          `build:`,
          `  echo building pkg-a`,
        ].join(`\n`));

        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/taskfile` as any), [
          `build:`,
          `  echo building pkg-b`,
        ].join(`\n`));

        // pkg-c uses *:build glob to depend on build task of all dependencies
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-c/taskfile` as any), [
          `build: *:build`,
          `  echo building pkg-c`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`debug`, `resolve-task`, `build`, {cwd: ppath.join(path, `packages/pkg-c` as any)});
        const result = JSON.parse(stdout);

        // *:build matches pkg-a:build and pkg-b:build (both are dependencies with build task)
        // Sequential ordering: pkg-a first, then pkg-b
        expect(result).toEqual({
          "pkg-a:build": [],
          "pkg-b:build": [`pkg-a:build`],
          "pkg-c:build": [`pkg-a:build`, `pkg-b:build`],
        });
      }),
    );

    test(
      `it should resolve glob pattern with parallel modifier`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`},
        [`packages/pkg-b`]: {name: `pkg-b`},
        [`packages/pkg-c`]: {name: `pkg-c`, dependencies: {[`pkg-a`]: `workspace:*`, [`pkg-b`]: `workspace:*`}},
      }, async ({path, run}) => {
        // pkg-a and pkg-b have build tasks
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-a/taskfile` as any), [
          `build:`,
          `  echo building pkg-a`,
        ].join(`\n`));

        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/taskfile` as any), [
          `build:`,
          `  echo building pkg-b`,
        ].join(`\n`));

        // pkg-c uses *:build& glob with parallel modifier
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-c/taskfile` as any), [
          `build: *:build&`,
          `  echo building pkg-c`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`debug`, `resolve-task`, `build`, {cwd: ppath.join(path, `packages/pkg-c` as any)});
        const result = JSON.parse(stdout);

        // *:build& matches pkg-a:build and pkg-b:build in parallel (no ordering between them)
        expect(result).toEqual({
          "pkg-a:build": [],
          "pkg-b:build": [],
          "pkg-c:build": [`pkg-a:build`, `pkg-b:build`],
        });
      }),
    );

    test(
      `it should only match dependencies that define the specified task`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`},
        [`packages/pkg-b`]: {name: `pkg-b`},
        [`packages/pkg-c`]: {name: `pkg-c`},
        [`packages/pkg-d`]: {name: `pkg-d`, dependencies: {[`pkg-a`]: `workspace:*`, [`pkg-b`]: `workspace:*`, [`pkg-c`]: `workspace:*`}},
      }, async ({path, run}) => {
        // pkg-a has a build task
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-a/taskfile` as any), [
          `build:`,
          `  echo building pkg-a`,
        ].join(`\n`));

        // pkg-b has a build task
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/taskfile` as any), [
          `build:`,
          `  echo building pkg-b`,
        ].join(`\n`));

        // pkg-c has NO build task (only a test task)
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-c/taskfile` as any), [
          `test:`,
          `  echo testing pkg-c`,
        ].join(`\n`));

        // pkg-d uses *:build& glob - should only match pkg-a and pkg-b, not pkg-c
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-d/taskfile` as any), [
          `build: *:build&`,
          `  echo building pkg-d`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`debug`, `resolve-task`, `build`, {cwd: ppath.join(path, `packages/pkg-d` as any)});
        const result = JSON.parse(stdout);

        // *:build& only matches pkg-a and pkg-b (pkg-c doesn't have a build task)
        expect(result).toEqual({
          "pkg-a:build": [],
          "pkg-b:build": [],
          "pkg-d:build": [`pkg-a:build`, `pkg-b:build`],
        });
      }),
    );

    test(
      `it should not run a task if one of its ancestors fails`,
      makeTemporaryEnv({
        name: `test-package`,
      }, async ({path, run}) => {
        const markerFile = ppath.join(path, `build-was-executed` as any);

        await xfs.writeFilePromise(ppath.join(path, `taskfile`), [
          `setup:`,
          `  exit 1`,
          ``,
          `build: setup`,
          `  touch ${markerFile}`,
        ].join(`\n`));

        await run(`install`);

        // The build task should fail because setup fails
        await expect(run(`build`)).rejects.toMatchObject({
          code: 1,
        });

        // The build task's script should never have been executed
        expect(xfs.existsSync(markerFile)).toBe(false);
      }),
    );

    test(
      `it should resolve tasks from included taskfiles`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`},
        [`packages/pkg-b`]: {name: `pkg-b`, dependencies: {[`pkg-a`]: `workspace:*`}},
      }, async ({path, run}) => {
        // pkg-a has a taskfile with a lint task
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-a/taskfile` as any), [
          `lint:`,
          `  echo linting pkg-a`,
        ].join(`\n`));

        // pkg-b includes pkg-a's taskfile and has its own build task
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/taskfile` as any), [
          `include pkg-a`,
          ``,
          `build:`,
          `  echo building pkg-b`,
        ].join(`\n`));

        await run(`install`);

        // The lint task should be available in pkg-b via include
        const {stdout} = await run(`debug`, `resolve-task`, `lint`, {cwd: ppath.join(path, `packages/pkg-b` as any)});
        const result = JSON.parse(stdout);

        expect(result).toEqual({
          "pkg-b:lint": [],
        });
      }),
    );

    test(
      `it should resolve tasks from included taskfiles with custom path`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`},
        [`packages/pkg-b`]: {name: `pkg-b`, dependencies: {[`pkg-a`]: `workspace:*`}},
      }, async ({path, run}) => {
        // pkg-a has a custom taskfile at tasks/lint.tasks
        await xfs.mkdirPromise(ppath.join(path, `packages/pkg-a/tasks` as any), {recursive: true});
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-a/tasks/lint.tasks` as any), [
          `lint:`,
          `  echo linting from custom path`,
        ].join(`\n`));

        // pkg-b includes pkg-a's custom taskfile
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/taskfile` as any), [
          `include pkg-a/tasks/lint.tasks`,
          ``,
          `build: lint`,
          `  echo building pkg-b`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`debug`, `resolve-task`, `build`, {cwd: ppath.join(path, `packages/pkg-b` as any)});
        const result = JSON.parse(stdout);

        expect(result).toEqual({
          "pkg-b:lint": [],
          "pkg-b:build": [`pkg-b:lint`],
        });
      }),
    );

    test(
      `it should fail when including a workspace that is not a dependency`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`},
        [`packages/pkg-b`]: {name: `pkg-b`}, // Note: pkg-a is NOT a dependency
      }, async ({path, run}) => {
        // pkg-a has a taskfile
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-a/taskfile` as any), [
          `lint:`,
          `  echo linting`,
        ].join(`\n`));

        // pkg-b tries to include pkg-a without declaring it as a dependency
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/taskfile` as any), [
          `include pkg-a`,
          ``,
          `build:`,
          `  echo building`,
        ].join(`\n`));

        await run(`install`);

        await expect(run(`debug`, `resolve-task`, `build`, {cwd: ppath.join(path, `packages/pkg-b` as any)})).rejects.toMatchObject({
          code: 1,
        });
      }),
    );

    test(
      `it should resolve included tasks that depend on other local tasks`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`},
        [`packages/pkg-b`]: {name: `pkg-b`, dependencies: {[`pkg-a`]: `workspace:*`}},
      }, async ({path, run}) => {
        // pkg-a has tasks with internal dependencies
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-a/taskfile` as any), [
          `setup:`,
          `  echo setup`,
          ``,
          `build: setup`,
          `  echo building`,
        ].join(`\n`));

        // pkg-b includes pkg-a's taskfile
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/taskfile` as any), [
          `include pkg-a`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`debug`, `resolve-task`, `build`, {cwd: ppath.join(path, `packages/pkg-b` as any)});
        const result = JSON.parse(stdout);

        // Both tasks should be available with their dependency relationship
        expect(result).toEqual({
          "pkg-b:setup": [],
          "pkg-b:build": [`pkg-b:setup`],
        });
      }),
    );

    test(
      `it should allow included tasks to be used as dependencies`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`},
        [`packages/pkg-b`]: {name: `pkg-b`, dependencies: {[`pkg-a`]: `workspace:*`}},
      }, async ({path, run}) => {
        // pkg-a has a lint task
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-a/taskfile` as any), [
          `lint:`,
          `  echo linting`,
        ].join(`\n`));

        // pkg-b includes pkg-a and defines a build task that depends on the included lint
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/taskfile` as any), [
          `include pkg-a`,
          ``,
          `build: lint`,
          `  echo building`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`debug`, `resolve-task`, `build`, {cwd: ppath.join(path, `packages/pkg-b` as any)});
        const result = JSON.parse(stdout);

        expect(result).toEqual({
          "pkg-b:lint": [],
          "pkg-b:build": [`pkg-b:lint`],
        });
      }),
    );

    test(
      `it should support multiple includes`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`},
        [`packages/pkg-b`]: {name: `pkg-b`},
        [`packages/pkg-c`]: {name: `pkg-c`, dependencies: {[`pkg-a`]: `workspace:*`, [`pkg-b`]: `workspace:*`}},
      }, async ({path, run}) => {
        // pkg-a has a lint task
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-a/taskfile` as any), [
          `lint:`,
          `  echo linting`,
        ].join(`\n`));

        // pkg-b has a typecheck task
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/taskfile` as any), [
          `typecheck:`,
          `  echo typechecking`,
        ].join(`\n`));

        // pkg-c includes both and defines build depending on both
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-c/taskfile` as any), [
          `include pkg-a`,
          `include pkg-b`,
          ``,
          `build: lint typecheck`,
          `  echo building`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`debug`, `resolve-task`, `build`, {cwd: ppath.join(path, `packages/pkg-c` as any)});
        const result = JSON.parse(stdout);

        expect(result).toEqual({
          "pkg-c:lint": [],
          "pkg-c:typecheck": [`pkg-c:lint`],
          "pkg-c:build": [`pkg-c:lint`, `pkg-c:typecheck`],
        });
      }),
    );

    test(
      `it should support scoped package includes`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/my-lib`]: {name: `@my-scope/my-lib`},
        [`packages/pkg-b`]: {name: `pkg-b`, dependencies: {[`@my-scope/my-lib`]: `workspace:*`}},
      }, async ({path, run}) => {
        // @my-scope/my-lib has a lint task
        await xfs.writeFilePromise(ppath.join(path, `packages/my-lib/taskfile` as any), [
          `lint:`,
          `  echo linting`,
        ].join(`\n`));

        // pkg-b includes the scoped package
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/taskfile` as any), [
          `include @my-scope/my-lib`,
          ``,
          `build: lint`,
          `  echo building`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`debug`, `resolve-task`, `build`, {cwd: ppath.join(path, `packages/pkg-b` as any)});
        const result = JSON.parse(stdout);

        expect(result).toEqual({
          "pkg-b:lint": [],
          "pkg-b:build": [`pkg-b:lint`],
        });
      }),
    );

    test(
      `it should support scoped package includes with custom path`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/my-lib`]: {name: `@my-scope/my-lib`},
        [`packages/pkg-b`]: {name: `pkg-b`, dependencies: {[`@my-scope/my-lib`]: `workspace:*`}},
      }, async ({path, run}) => {
        // @my-scope/my-lib has a custom taskfile
        await xfs.mkdirPromise(ppath.join(path, `packages/my-lib/tasks` as any), {recursive: true});
        await xfs.writeFilePromise(ppath.join(path, `packages/my-lib/tasks/common.tasks` as any), [
          `lint:`,
          `  echo linting`,
        ].join(`\n`));

        // pkg-b includes the scoped package with custom path
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/taskfile` as any), [
          `include @my-scope/my-lib/tasks/common.tasks`,
          ``,
          `build: lint`,
          `  echo building`,
        ].join(`\n`));

        await run(`install`);

        const {stdout} = await run(`debug`, `resolve-task`, `build`, {cwd: ppath.join(path, `packages/pkg-b` as any)});
        const result = JSON.parse(stdout);

        expect(result).toEqual({
          "pkg-b:lint": [],
          "pkg-b:build": [`pkg-b:lint`],
        });
      }),
    );
  });
});
