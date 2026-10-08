import {ppath, xfs, PortablePath} from '@yarnpkg/fslib';

const writeTaskfile = async (path: PortablePath, rel: string, lines: Array<string>) => {
  await xfs.writeFilePromise(ppath.join(path, rel as PortablePath), lines.join(`\n`));
};

const parseEvents = (stdout: string) => stdout.trim().split(`\n`).filter(Boolean).map(line => JSON.parse(line));

const startedOrder = (events: Array<any>) => events
  .filter(e => e.type === `task-started`)
  .map(e => e.taskId);

describe(`Commands`, () => {
  describe(`tasks run (root workspace defaults)`, () => {
    test(
      `it should run package.json scripts through @workspaces defaults, honoring ^ dependencies`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`, scripts: {build: `echo build-a`}},
        [`packages/pkg-b`]: {name: `pkg-b`, scripts: {build: `echo build-b`}, dependencies: {[`pkg-a`]: `workspace:*`}},
        [`packages/pkg-c`]: {name: `pkg-c`, scripts: {build: `echo build-c`}, devDependencies: {[`pkg-b`]: `workspace:*`}},
      }, async ({path, run, runSwitch}) => {
        await writeTaskfile(path, `taskfile`, [
          `@workspaces`,
          `build: ^build`,
        ]);

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `build`, {cwd: ppath.join(path, `packages/pkg-c` as PortablePath)});
        expect(stdout).toEqual(`build-a\nbuild-b\nbuild-c\n`);
      }),
    );

    test(
      `it should not deadlock when ^ dependencies form a diamond`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        // top depends on both mid and base, and mid depends on base: expanding
        // `^build` must not order base and mid against each other arbitrarily
        [`packages/base`]: {name: `base`, scripts: {build: `echo build-base`}},
        [`packages/mid`]: {name: `mid`, scripts: {build: `echo build-mid`}, dependencies: {[`base`]: `workspace:*`}},
        [`packages/top`]: {name: `top`, scripts: {build: `echo build-top`}, dependencies: {[`base`]: `workspace:*`, [`mid`]: `workspace:*`}},
      }, async ({path, run, runSwitch}) => {
        await writeTaskfile(path, `taskfile`, [
          `@workspaces`,
          `build: ^build`,
        ]);

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `build`, {cwd: ppath.join(path, `packages/top` as PortablePath)});
        expect(stdout).toEqual(`build-base\nbuild-mid\nbuild-top\n`);
      }),
    );

    test(
      `it should treat workspaces without the matching script as no-ops that still propagate ordering`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`, scripts: {build: `echo build-a`}},
        // pkg-b has no build script, but must still order pkg-a before pkg-c
        [`packages/pkg-b`]: {name: `pkg-b`, dependencies: {[`pkg-a`]: `workspace:*`}},
        [`packages/pkg-c`]: {name: `pkg-c`, scripts: {build: `echo build-c`}, dependencies: {[`pkg-b`]: `workspace:*`}},
      }, async ({path, run, runSwitch}) => {
        await writeTaskfile(path, `taskfile`, [
          `@workspaces`,
          `build: ^build`,
        ]);

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `build`, {cwd: ppath.join(path, `packages/pkg-c` as PortablePath)});
        expect(stdout).toEqual(`build-a\nbuild-c\n`);
      }),
    );

    test(
      `it should support local dependencies in defaults (check: test:ci)`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`, scripts: {build: `echo build-a`, [`test:ci`]: `echo test-a`}},
        [`packages/pkg-b`]: {name: `pkg-b`, scripts: {[`test:ci`]: `echo test-b`}, dependencies: {[`pkg-a`]: `workspace:*`}},
      }, async ({path, run, runSwitch}) => {
        await writeTaskfile(path, `taskfile`, [
          `@workspaces`,
          `build: ^build`,
          ``,
          `@workspaces`,
          `test:ci: ^build ^test:ci`,
          ``,
          `@workspaces`,
          `check: test:ci`,
        ]);

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `--json`, `check`, {cwd: ppath.join(path, `packages/pkg-b` as PortablePath)});
        const order = startedOrder(parseEvents(stdout));

        expect(order).toContain(`pkg-a:build`);
        expect(order).toContain(`pkg-a:test:ci`);
        expect(order[order.length - 1]).toEqual(`pkg-b:test:ci`);
        // check itself has no script and no package.json script: no process started
        expect(order).not.toContain(`pkg-b:check`);
      }),
    );

    test(
      `it should let local workspace taskfiles override root defaults`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`, scripts: {build: `echo build-a`}},
        [`packages/pkg-b`]: {name: `pkg-b`, scripts: {build: `echo build-b`}, dependencies: {[`pkg-a`]: `workspace:*`}},
      }, async ({path, run, runSwitch}) => {
        await writeTaskfile(path, `taskfile`, [
          `@workspaces`,
          `build: ^build`,
        ]);

        await writeTaskfile(path, `packages/pkg-a/taskfile`, [
          `build:`,
          `  echo custom-a`,
        ]);

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `build`, {cwd: ppath.join(path, `packages/pkg-b` as PortablePath)});
        expect(stdout).toEqual(`custom-a\nbuild-b\n`);
      }),
    );

    test(
      `it should forward extra arguments to the package.json script`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`, scripts: {build: `echo build-a`}},
      }, async ({path, run, runSwitch}) => {
        await writeTaskfile(path, `taskfile`, [
          `@workspaces`,
          `build: ^build`,
        ]);

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `build`, `--flag`, {cwd: ppath.join(path, `packages/pkg-a` as PortablePath)});
        expect(stdout).toEqual(`build-a --flag\n`);
      }),
    );

    test(
      `it should keep 'yarn run <script>' running the package.json script directly`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`, scripts: {build: `echo build-a`}},
        [`packages/pkg-b`]: {name: `pkg-b`, scripts: {build: `echo build-b`}, dependencies: {[`pkg-a`]: `workspace:*`}},
      }, async ({path, run}) => {
        await writeTaskfile(path, `taskfile`, [
          `@workspaces`,
          `build: ^build`,
        ]);

        await run(`install`);

        const {stdout} = await run(`run`, `build`, {cwd: ppath.join(path, `packages/pkg-b` as PortablePath)});
        expect(stdout).toEqual(`build-b\n`);
      }),
    );

    test(
      `it should not expose @workspaces defaults as tasks of the root workspace itself`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
        scripts: {build: `echo build-root`},
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`, scripts: {build: `echo build-a`}},
      }, async ({path, run, runSwitch}) => {
        await writeTaskfile(path, `taskfile`, [
          `@workspaces`,
          `build: ^build`,
          ``,
          `hello:`,
          `  echo hello-root`,
        ]);

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `hello`);
        expect(stdout).toEqual(`hello-root\n`);

        await expect(runSwitch(`tasks`, `run`, `--standalone`, `build`)).rejects.toThrow();
      }),
    );
  });
});
