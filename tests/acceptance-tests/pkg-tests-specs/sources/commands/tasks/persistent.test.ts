import {npath, ppath, xfs, PortablePath} from '@yarnpkg/fslib';
import * as cp                           from 'child_process';
import {delimiter}                       from 'path';

const getYarnBinBinaryPath = () =>
  process.env.TEST_BINARY
    ?? require.resolve(`${__dirname}/../../../../../../target/release/yarn-bin`);

function spawnYarnBin(cwd: PortablePath, home: PortablePath, args: Array<string>) {
  const child = cp.spawn(getYarnBinBinaryPath(), args, {
    cwd: npath.fromPortablePath(cwd),
    env: {
      HOME: npath.fromPortablePath(home),
      PATH: `${npath.fromPortablePath(home)}/bin${delimiter}${process.env.PATH}`,
      YARN_IS_TEST_ENV: `true`,
      YARN_GLOBAL_FOLDER: `${npath.fromPortablePath(home)}/.yarn/global`,
      YARN_ENABLE_TELEMETRY: `0`,
      YARN_ENABLE_PROGRESS_BARS: `false`,
      FORCE_COLOR: `0`,
      NODE_OPTIONS: ``,
      YARN_DAEMON_DEFAULT_WARMUP_PERIOD: `500ms`,
    },
  });

  let stdout = ``;
  child.stdout?.on(`data`, (d: Buffer) => {
    stdout += d.toString();
  });
  child.stderr?.on(`data`, (d: Buffer) => {
    stdout += d.toString();
  });

  const closed = new Promise<number>(resolve => {
    child.on(`close`, code => resolve(code ?? 1));
  });

  return {child, closed, getStdout: () => stdout};
}

const waitFor = async (check: () => boolean, timeout = 10000) => {
  const start = Date.now();
  while (!check()) {
    if (Date.now() - start > timeout)
      throw new Error(`Timeout`);

    await new Promise(resolve => setTimeout(resolve, 50));
  }
};

const isAlive = (pid: number) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};

describe(`Commands`, () => {
  describe(`tasks run (persistent tasks with ^ dependencies)`, () => {
    test(
      `it should build the dependencies, then keep the @long-lived dev task running until interrupted`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/lib`]: {name: `lib`, scripts: {build: `sleep 0.3 && echo built > out.txt && echo build-lib`}},
        [`packages/app`]: {name: `app`, scripts: {dev: `cat ../lib/out.txt && echo $$ > ../../dev.pid && echo dev-started && sleep 60`}, dependencies: {[`lib`]: `workspace:*`}},
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile` as PortablePath), [
          `@workspaces`,
          `build: ^build`,
          ``,
          `@workspaces`,
          `@long-lived`,
          `dev: ^build`,
        ].join(`\n`));

        await run(`install`);

        const proc = spawnYarnBin(path, ppath.dirname(path), [`tasks`, `run`, `--standalone`, `--from`, `app`, `dev`]);

        await waitFor(() => proc.getStdout().includes(`dev-started`));

        // The dependency built before the dev server started, and the server read its output
        expect(proc.getStdout()).toMatch(/\[lib:build\]: build-lib[\s\S]*\[app:dev\]: built[\s\S]*\[app:dev\]: dev-started/);

        // The server keeps running
        await new Promise(resolve => setTimeout(resolve, 1000));
        expect(proc.child.exitCode).toBeNull();

        const pid = parseInt(await xfs.readFilePromise(ppath.join(path, `dev.pid` as PortablePath), `utf8`), 10);
        expect(isAlive(pid)).toBe(true);

        proc.child.kill(`SIGINT`);
        expect(await proc.closed).toEqual(130);

        // A standalone run can't detach: the server must be stopped with it
        await waitFor(() => !isAlive(pid), 5000);
      }),
    );

    test(
      `SIGTERM should stop the run and its long-lived tasks`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/app`]: {name: `app`, scripts: {dev: `echo $$ > ../../dev.pid && echo dev-started && sleep 60`}},
      }, async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `taskfile` as PortablePath), [
          `@workspaces`,
          `@long-lived`,
          `dev: ^build`,
        ].join(`\n`));

        await run(`install`);

        const proc = spawnYarnBin(path, ppath.dirname(path), [`tasks`, `run`, `--standalone`, `-A`, `dev`]);
        await waitFor(() => proc.getStdout().includes(`dev-started`));

        const pid = parseInt(await xfs.readFilePromise(ppath.join(path, `dev.pid` as PortablePath), `utf8`), 10);

        proc.child.kill(`SIGTERM`);
        expect(await proc.closed).toEqual(143);

        await waitFor(() => !isAlive(pid), 5000);
      }),
    );
  });
});
