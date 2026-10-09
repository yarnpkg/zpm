import {ppath, xfs, PortablePath} from '@yarnpkg/fslib';

// util <- lib <- app ; app also depends on ui ; ui <- other-app ; tool is independent.
// Selecting `lib` with dependents + dependencies must include `ui` (a
// dependency of the dependent `app`), which the dependencies of `lib` alone
// wouldn't.
const closureEnv = makeTemporaryMonorepoEnv.bind(null, {
  name: `root`,
  workspaces: [`packages/*`],
}, {
  [`packages/util`]: {name: `util`, scripts: {build: `echo util`}},
  [`packages/lib`]: {name: `lib`, scripts: {build: `echo lib`}, dependencies: {[`util`]: `workspace:*`}},
  [`packages/ui`]: {name: `ui`, scripts: {build: `echo ui`}},
  [`packages/app`]: {name: `app`, scripts: {build: `echo app`}, dependencies: {[`lib`]: `workspace:*`, [`ui`]: `workspace:*`}},
  [`packages/other-app`]: {name: `other-app`, scripts: {build: `echo other-app`}, dependencies: {[`ui`]: `workspace:*`}},
  [`packages/tool`]: {name: `tool`, scripts: {build: `echo tool`}},
});

describe(`Commands`, () => {
  describe(`dependents + dependencies closure`, () => {
    test(
      `workspaces foreach --follow-dependents --follow-dependencies should follow dependencies from the dependents too`,
      closureEnv(async ({run}) => {
        await run(`install`);

        const {stdout} = await run(`workspaces`, `foreach`, `--from`, `lib`, `--follow-dependents`, `--follow-dependencies`, `exec`, `node`, `-p`, `require("./package.json").name`);
        expect(stdout.trim().split(`\n`).sort()).toEqual([`app`, `lib`, `ui`, `util`]);
      }),
    );

    test(
      `workspaces foreach --follow-dependents alone should still only add dependents`,
      closureEnv(async ({run}) => {
        await run(`install`);

        const {stdout} = await run(`workspaces`, `foreach`, `--from`, `lib`, `--follow-dependents`, `exec`, `node`, `-p`, `require("./package.json").name`);
        expect(stdout.trim().split(`\n`).sort()).toEqual([`app`, `lib`]);
      }),
    );

    test(
      `tasks run --with-dependents --with-dependencies should select the same closure`,
      closureEnv(async ({path, run, runSwitch}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile` as PortablePath), `@workspaces\nbuild:\n`);
        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `--json`, `--from`, `lib`, `--with-dependents`, `--with-dependencies`, `build`);
        const started = stdout.trim().split(`\n`).map(line => JSON.parse(line)).filter(e => e.type === `task-started`).map(e => e.taskId);
        expect(started.sort()).toEqual([`app:build`, `lib:build`, `ui:build`, `util:build`]);
      }),
    );
  });
});
