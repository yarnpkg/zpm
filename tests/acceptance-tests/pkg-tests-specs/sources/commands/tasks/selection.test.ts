import {ppath, xfs, PortablePath} from '@yarnpkg/fslib';

const parseEvents = (stdout: string) => stdout.trim().split(`\n`).filter(Boolean).map(line => JSON.parse(line));

const eventsOf = (events: Array<any>, type: string) => events
  .filter(e => e.type === type)
  .map(e => e.taskId);

// a <- b <- c ; d (independent) ; e has no build script
const monorepo = (extra: Record<string, any> = {}) => makeTemporaryMonorepoEnv.bind(null, {
  name: `root`,
  workspaces: [`packages/*`],
}, {
  [`packages/pkg-a`]: {name: `pkg-a`, scripts: {build: `echo build-a`, typecheck: `echo tc-a`}},
  [`packages/pkg-b`]: {name: `pkg-b`, scripts: {build: `echo build-b`, typecheck: `echo tc-b`}, dependencies: {[`pkg-a`]: `workspace:*`}},
  [`packages/pkg-c`]: {name: `pkg-c`, scripts: {build: `echo build-c`}, devDependencies: {[`pkg-b`]: `workspace:*`}},
  [`packages/pkg-d`]: {name: `pkg-d`, scripts: {build: `echo build-d`}},
  [`packages/pkg-e`]: {name: `pkg-e`},
  ...extra,
});

const rootTaskfile = [
  `@defaultTask`,
  `build: *:build`,
  ``,
  `@defaultTask`,
  `typecheck:`,
];

async function setup(path: PortablePath, run: any) {
  await xfs.writeFilePromise(ppath.join(path, `taskfile` as PortablePath), rootTaskfile.join(`\n`));
  await run(`install`);
}

async function runJson(runSwitch: any, args: Array<string>, opts: Record<string, any> = {}) {
  const result = await runSwitch(`tasks`, `run`, `--standalone`, `--json`, ...args, opts).catch((e: any) => e);
  return {code: result.code ?? 0, events: parseEvents(result.stdout)};
}

describe(`Commands`, () => {
  describe(`tasks run (workspace selection)`, () => {
    test(
      `-A should run the task in every workspace declaring it, in dependency order`,
      monorepo()(async ({path, run, runSwitch}) => {
        await setup(path, run);

        const {code, events} = await runJson(runSwitch, [`-A`, `build`]);
        expect(code).toEqual(0);

        const started = eventsOf(events, `task-started`);
        expect([...started].sort()).toEqual([`pkg-a:build`, `pkg-b:build`, `pkg-c:build`, `pkg-d:build`]);
        expect(started.indexOf(`pkg-a:build`)).toBeLessThan(started.indexOf(`pkg-b:build`));
        expect(started.indexOf(`pkg-b:build`)).toBeLessThan(started.indexOf(`pkg-c:build`));

        // Each task runs exactly once even though pkg-a:build is a dependency of several targets
        expect(started.filter(id => id === `pkg-a:build`)).toHaveLength(1);
      }),
    );

    test(
      `--from should select workspaces by ident glob and run their *: dependencies`,
      monorepo()(async ({path, run, runSwitch}) => {
        await setup(path, run);

        const {events} = await runJson(runSwitch, [`--from`, `pkg-c`, `build`]);
        expect(eventsOf(events, `task-started`)).toEqual([`pkg-a:build`, `pkg-b:build`, `pkg-c:build`]);
      }),
    );

    test(
      `--from should accept ./-prefixed path globs`,
      monorepo()(async ({path, run, runSwitch}) => {
        await setup(path, run);

        const {events} = await runJson(runSwitch, [`--from`, `./packages/pkg-{a,d}`, `build`]);
        expect(eventsOf(events, `task-started`).sort()).toEqual([`pkg-a:build`, `pkg-d:build`]);
      }),
    );

    test(
      `--recursive should apply to the current workspace when no seed is given`,
      monorepo()(async ({path, run, runSwitch}) => {
        await setup(path, run);

        // Without *:build in typecheck, --recursive is what pulls the dependencies in
        const {events} = await runJson(runSwitch, [`--recursive`, `typecheck`], {cwd: ppath.join(path, `packages/pkg-c` as PortablePath)});
        expect(eventsOf(events, `task-started`).sort()).toEqual([`pkg-a:typecheck`, `pkg-b:typecheck`]);
      }),
    );

    test(
      `--follow-dependencies should add the dependencies of the selection`,
      monorepo()(async ({path, run, runSwitch}) => {
        await setup(path, run);

        const {events} = await runJson(runSwitch, [`--from`, `pkg-b`, `--follow-dependencies`, `typecheck`]);
        expect(eventsOf(events, `task-started`).sort()).toEqual([`pkg-a:typecheck`, `pkg-b:typecheck`]);
      }),
    );

    test(
      `--followed-dependencies should restrict the dependency kinds being followed`,
      monorepo()(async ({path, run, runSwitch}) => {
        await setup(path, run);

        // pkg-c only depends on pkg-b through its devDependencies
        const {events: prod} = await runJson(runSwitch, [`--from`, `pkg-c`, `--follow-dependencies`, `--followed-dependencies`, `prod`, `--only`, `build`]);
        expect(eventsOf(prod, `task-started`)).toEqual([`pkg-c:build`]);

        const {events: dev} = await runJson(runSwitch, [`--from`, `pkg-c`, `--follow-dependencies`, `--followed-dependencies`, `dev`, `--only`, `build`]);
        expect(eventsOf(dev, `task-started`)).toEqual([`pkg-b:build`, `pkg-c:build`]);
      }),
    );

    test(
      `--follow-dependents should add the dependents of the selection`,
      monorepo()(async ({path, run, runSwitch}) => {
        await setup(path, run);

        const {events} = await runJson(runSwitch, [`--from`, `pkg-b`, `--follow-dependents`, `--only`, `build`]);
        expect(eventsOf(events, `task-started`)).toEqual([`pkg-b:build`, `pkg-c:build`]);
      }),
    );

    test(
      `--no-private should skip private workspaces`,
      monorepo({
        [`packages/pkg-d`]: {name: `pkg-d`, private: true, scripts: {build: `echo build-d`}},
      })(async ({path, run, runSwitch}) => {
        await setup(path, run);

        const {events} = await runJson(runSwitch, [`-A`, `--no-private`, `build`]);
        expect(eventsOf(events, `task-started`).sort()).toEqual([`pkg-a:build`, `pkg-b:build`, `pkg-c:build`]);
      }),
    );

    test(
      `--only should skip *: dependencies outside of the selection`,
      monorepo()(async ({path, run, runSwitch}) => {
        await setup(path, run);

        const {events} = await runJson(runSwitch, [`--from`, `pkg-c`, `--only`, `build`]);
        expect(eventsOf(events, `task-started`)).toEqual([`pkg-c:build`]);
      }),
    );

    test(
      `--include and --exclude should filter the selection`,
      monorepo()(async ({path, run, runSwitch}) => {
        await setup(path, run);

        const {events} = await runJson(runSwitch, [`-A`, `--exclude`, `pkg-{a,b,c}`, `build`]);
        expect(eventsOf(events, `task-started`)).toEqual([`pkg-d:build`]);

        const {events: included} = await runJson(runSwitch, [`--include`, `./packages/pkg-d`, `build`]);
        expect(eventsOf(included, `task-started`)).toEqual([`pkg-d:build`]);
      }),
    );

    test(
      `it should run multiple comma-separated tasks in a single graph`,
      monorepo()(async ({path, run, runSwitch}) => {
        await setup(path, run);

        const {events} = await runJson(runSwitch, [`--from`, `pkg-b`, `build,typecheck`]);
        expect(eventsOf(events, `task-started`).sort()).toEqual([`pkg-a:build`, `pkg-b:build`, `pkg-b:typecheck`]);
      }),
    );

    test(
      `it should succeed with a message when nothing matches`,
      monorepo()(async ({path, run, runSwitch}) => {
        await setup(path, run);

        const {code} = await runJson(runSwitch, [`--from`, `pkg-e`, `--only`, `build`]);
        expect(code).toEqual(0);
      }),
    );

    test(
      `it should cancel the run on the first failure, and report its exit code`,
      monorepo({
        [`packages/pkg-a`]: {name: `pkg-a`, scripts: {build: `exit 3`}},
        [`packages/pkg-d`]: {name: `pkg-d`, scripts: {build: `sleep 5 && echo build-d`}},
      })(async ({path, run, runSwitch}) => {
        await setup(path, run);

        const start = Date.now();
        const {code, events} = await runJson(runSwitch, [`-A`, `build`]);

        expect(code).toEqual(3);
        expect(Date.now() - start).toBeLessThan(4500);
        expect(eventsOf(events, `task-started`)).not.toContain(`pkg-b:build`);
        expect(eventsOf(events, `task-cancelled`)).toEqual(expect.arrayContaining([`pkg-b:build`, `pkg-c:build`]));
      }),
    );

    test(
      `it should report the exit code of a failing dependency`,
      monorepo({
        [`packages/pkg-a`]: {name: `pkg-a`, scripts: {build: `exit 3`}},
      })(async ({path, run, runSwitch}) => {
        await setup(path, run);

        // pkg-a:build only runs as a dependency of pkg-b:build
        const {code} = await runJson(runSwitch, [`--from`, `pkg-b`, `--from`, `pkg-d`, `build`]);
        expect(code).toEqual(3);
      }),
    );

    test(
      `--continue should keep running independent tasks after a failure`,
      monorepo({
        [`packages/pkg-a`]: {name: `pkg-a`, scripts: {build: `exit 3`}},
        [`packages/pkg-d`]: {name: `pkg-d`, scripts: {build: `sleep 1 && echo build-d`}},
      })(async ({path, run, runSwitch}) => {
        await setup(path, run);

        const {code, events} = await runJson(runSwitch, [`-A`, `--continue`, `build`]);

        expect(code).toEqual(3);
        expect(events).toContainEqual({type: `task-completed`, taskId: `pkg-d:build`, exitCode: 0});
        expect(eventsOf(events, `task-started`)).not.toContain(`pkg-b:build`);
      }),
    );

    test(
      `--errors-only should only print the output of failed tasks`,
      monorepo({
        [`packages/pkg-d`]: {name: `pkg-d`, scripts: {build: `echo oops-d && exit 2`}},
      })(async ({path, run, runSwitch}) => {
        await setup(path, run);

        const result = await runSwitch(`tasks`, `run`, `--standalone`, `-A`, `--continue`, `--errors-only`, `build`).catch((e: any) => e);
        expect(result.code).toEqual(2);
        expect(result.stdout).toContain(`[pkg-d:build]: oops-d`);
        expect(result.stdout).not.toContain(`build-a`);
        expect(result.stdout).toContain(`4 tasks succeeded, 1 failed`);
      }),
    );

    test(
      `taskConcurrency should limit the number of concurrent processes`,
      monorepo({
        [`packages/pkg-a`]: {name: `pkg-a`, scripts: {build: `mkdir lock-a 2>/dev/null; ls -d ../*/lock-* | wc -l | tr -d ' ' > ../../conc-a; sleep 0.3; rmdir lock-a`}},
        [`packages/pkg-d`]: {name: `pkg-d`, scripts: {build: `mkdir lock-d 2>/dev/null; ls -d ../*/lock-* | wc -l | tr -d ' ' > ../../conc-d; sleep 0.3; rmdir lock-d`}},
      })(async ({path, run, runSwitch}) => {
        await setup(path, run);

        const {code} = await runJson(runSwitch, [`--from`, `pkg-{a,d}`, `build`], {env: {YARN_TASK_CONCURRENCY: `1`}});
        expect(code).toEqual(0);

        expect((await xfs.readFilePromise(ppath.join(path, `conc-a` as PortablePath), `utf8`)).trim()).toEqual(`1`);
        expect((await xfs.readFilePromise(ppath.join(path, `conc-d` as PortablePath), `utf8`)).trim()).toEqual(`1`);
      }),
    );

    test(
      `taskConcurrency should be shared by the runs of the background daemon`,
      monorepo({
        [`packages/pkg-a`]: {name: `pkg-a`, scripts: {build: `mkdir lock-a 2>/dev/null; sleep 0.5; ls -d ../*/lock-* | wc -l | tr -d ' ' > ../../conc-a; rmdir lock-a`}},
        [`packages/pkg-d`]: {name: `pkg-d`, scripts: {build: `mkdir lock-d 2>/dev/null; sleep 0.5; ls -d ../*/lock-* | wc -l | tr -d ' ' > ../../conc-d; rmdir lock-d`}},
      })(async ({path, run, runSwitch}) => {
        await setup(path, run);

        const env = {YARN_TASK_CONCURRENCY: `1`};

        try {
          // Start the daemon first, so both runs go through the same one
          await runSwitch(`tasks`, `run`, `--no-standalone`, `--from`, `pkg-e`, `build`, {env});

          await Promise.all([
            runSwitch(`tasks`, `run`, `--no-standalone`, `--from`, `pkg-a`, `--only`, `build`, {env}),
            runSwitch(`tasks`, `run`, `--no-standalone`, `--from`, `pkg-d`, `--only`, `build`, {env}),
          ]);

          expect((await xfs.readFilePromise(ppath.join(path, `conc-a` as PortablePath), `utf8`)).trim()).toEqual(`1`);
          expect((await xfs.readFilePromise(ppath.join(path, `conc-d` as PortablePath), `utf8`)).trim()).toEqual(`1`);
        } finally {
          await runSwitch(`switch`, `daemon`, `--kill-all`);
        }
      }),
    );

    test(
      `long-lived tasks shouldn't count toward taskConcurrency`,
      monorepo()(async ({path, run, runSwitch}) => {
        await setup(path, run);
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/taskfile` as PortablePath), [
          `@long-lived`,
          `server:`,
          `  sleep 30`,
          ``,
          `client: server`,
          `  echo client-b`,
        ].join(`\n`));

        const {code, events} = await runJson(runSwitch, [`--from`, `pkg-b`, `client`], {env: {YARN_TASK_CONCURRENCY: `1`}});
        expect(code).toEqual(0);
        expect(eventsOf(events, `task-started`)).toContain(`pkg-b:client`);
      }),
    );

    test(
      `tasks waiting on a nested run shouldn't count toward taskConcurrency`,
      monorepo()(async ({path, run, runSwitch}) => {
        await setup(path, run);
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-d/taskfile` as PortablePath), [
          `outer:`,
          `  yarn tasks run --from pkg-a --only build`,
        ].join(`\n`));

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `--from`, `pkg-d`, `outer`, {env: {YARN_TASK_CONCURRENCY: `1`}});
        expect(stdout).toContain(`build-a`);
      }),
    );

    test(
      `--since should run the task in changed workspaces, and their dependents with --recursive`,
      monorepo()(async ({path, run, runSwitch, git}: any) => {
        await setup(path, run);

        await xfs.writeFilePromise(ppath.join(path, `.gitignore` as PortablePath), `.yarn\n`);
        const sh = async (...args: Array<string>) => {
          const {execFileSync} = require(`child_process`);
          execFileSync(`git`, args, {cwd: path, env: {...process.env, GIT_AUTHOR_NAME: `a`, GIT_AUTHOR_EMAIL: `a@b`, GIT_COMMITTER_NAME: `a`, GIT_COMMITTER_EMAIL: `a@b`}});
        };

        await sh(`init`, `-q`, `-b`, `main`);
        await sh(`add`, `-A`);
        await sh(`commit`, `-q`, `-m`, `init`);

        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/index.js` as PortablePath), `// change\n`);

        const {events} = await runJson(runSwitch, [`--since=main`, `--only`, `build`]);
        expect(eventsOf(events, `task-started`)).toEqual([`pkg-b:build`]);

        const {events: recursive} = await runJson(runSwitch, [`--since=main`, `--recursive`, `--only`, `build`]);
        expect(eventsOf(recursive, `task-started`)).toEqual([`pkg-b:build`, `pkg-c:build`]);

        // Without a ref, --since uses the configured base refs (main/master)
        const {events: affected} = await runJson(runSwitch, [`--since`, `--recursive`, `--only`, `build`]);
        expect(eventsOf(affected, `task-started`)).toEqual([`pkg-b:build`, `pkg-c:build`]);
      }),
    );

    test(
      `--only runs shouldn't change the dependencies of concurrent runs`,
      monorepo({
        [`packages/pkg-a`]: {name: `pkg-a`, scripts: {build: `sleep 2 && echo build-a`}},
      })(async ({path, run, runSwitch}) => {
        await setup(path, run);

        try {
          // pkg-a:build takes a while; meanwhile an --only run resolves
          // pkg-b:build without its *:build dependency
          const full = runSwitch(`tasks`, `run`, `--no-standalone`, `--json`, `--from`, `pkg-b`, `build`);
          await new Promise(resolve => setTimeout(resolve, 500));
          await runSwitch(`tasks`, `run`, `--no-standalone`, `--json`, `--from`, `pkg-b`, `--only`, `build`);

          const events = parseEvents((await full).stdout);
          const order = events
            .filter(event => event.type === `task-started` || event.type === `task-completed`)
            .map(event => `${event.type}:${event.taskId}`);

          expect(order.indexOf(`task-completed:pkg-a:build`)).toBeGreaterThanOrEqual(0);
          expect(order.indexOf(`task-completed:pkg-a:build`)).toBeLessThan(order.indexOf(`task-started:pkg-b:build`));
        } finally {
          await runSwitch(`switch`, `daemon`, `--kill-all`);
        }
      }),
    );

    test(
      `--only should apply to long-lived targets`,
      monorepo({
        [`packages/pkg-a`]: {name: `pkg-a`, scripts: {build: `touch ../../built-a`}},
      })(async ({path, run, runSwitch}) => {
        await setup(path, run);
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/taskfile` as PortablePath), [
          `@long-lived`,
          `dev: *:build`,
          `  echo dev-b`,
        ].join(`\n`));

        await runJson(runSwitch, [`--from`, `pkg-b`, `--only`, `dev`]);
        expect(xfs.existsSync(ppath.join(path, `built-a` as PortablePath))).toEqual(false);
      }),
    );

    test(
      `it should run multi-workspace tasks through the background daemon too`,
      monorepo()(async ({path, run, runSwitch}) => {
        await setup(path, run);

        try {
          const {stdout} = await runSwitch(`tasks`, `run`, `--no-standalone`, `--from`, `pkg-b`, `build`);
          expect(stdout).toEqual(`[pkg-a:build]: build-a\n[pkg-b:build]: build-b\n`);
        } finally {
          await runSwitch(`switch`, `daemon`, `--kill-all`);
        }
      }),
    );
  });
});
