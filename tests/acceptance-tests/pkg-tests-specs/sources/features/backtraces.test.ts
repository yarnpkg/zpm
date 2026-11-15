import {ppath, xfs} from '@yarnpkg/fslib';

// Capturing a backtrace walks the whole stack, which is very expensive in
// static builds; RUST_BACKTRACE=1 used to make installs an order of
// magnitude slower because every expected I/O error (a file probed before
// being created, etc) captured one. The test env sets RUST_BACKTRACE=1.
const linkerConfigurations = [{
  name: `pnp`,
  config: {nodeLinker: `pnp`},
}, {
  name: `pnpm`,
  config: {nodeLinker: `pnpm`},
}, {
  name: `node-modules (classic)`,
  config: {nodeLinker: `node-modules`, nmMode: `classic`},
}, {
  name: `node-modules (hardlinks-local)`,
  config: {nodeLinker: `node-modules`, nmMode: `hardlinks-local`},
}, {
  name: `node-modules (hardlinks-global)`,
  config: {nodeLinker: `node-modules`, nmMode: `hardlinks-global`},
}];

const reportEnv = {
  YARN_TEST_REPORT_BACKTRACES: `1`,
};

describe(`Features`, () => {
  describe(`Backtraces`, () => {
    for (const {name, config} of linkerConfigurations) {
      test(
        `it shouldn't capture backtraces for expected errors when installing with the ${name} linker`,
        makeTemporaryEnv({
          dependencies: {
            [`one-fixed-dep`]: `1.0.0`,
            [`has-bin-entries`]: `1.0.0`,
            [`no-deps-scripted`]: `1.0.0`,
          },
        }, config, async ({path, run}) => {
          const firstInstall = await run(`install`, {env: reportEnv});
          expect(firstInstall.stderr).toContain(`Captured backtraces: 0\n`);

          // Warm install: the cache is there, but every file of every
          // package must be linked again
          await xfs.removePromise(ppath.join(path, `node_modules`));
          await xfs.removePromise(ppath.join(path, `.pnp.cjs`));

          const secondInstall = await run(`install`, {env: reportEnv});
          expect(secondInstall.stderr).toContain(`Captured backtraces: 0\n`);

          // No-op install: everything is already in place
          const thirdInstall = await run(`install`, {env: reportEnv});
          expect(thirdInstall.stderr).toContain(`Captured backtraces: 0\n`);
        }),
      );
    }

    test(
      `it should print a backtrace for reported I/O errors when RUST_BACKTRACE is set`,
      makeTemporaryEnv({}, async ({path, run}) => {
        // Reading it fails with EISDIR
        await xfs.mkdirPromise(ppath.join(path, `.yarnrc.yml`));

        const result = await run(`install`, {env: reportEnv}).catch(error => error);

        expect(result.code).toEqual(1);
        expect(result.stdout).toContain(`I/O error (Is a directory`);
        expect(result.stdout).toMatch(/^ +\d+: .*try_read$/m);
        expect(result.stdout).not.toContain(`Run with RUST_BACKTRACE=1 to get a backtrace`);

        // Proves that the counter used by the tests above sees the captures
        expect(result.stderr).toMatch(/Captured backtraces: [1-9]/);
      }),
    );

    test(
      `it shouldn't print a backtrace for reported I/O errors when RUST_BACKTRACE isn't set`,
      makeTemporaryEnv({}, async ({path, run}) => {
        await xfs.mkdirPromise(ppath.join(path, `.yarnrc.yml`));

        const result = await run(`install`, {env: {...reportEnv, RUST_BACKTRACE: undefined}}).catch(error => error);

        expect(result.code).toEqual(1);
        expect(result.stdout).toContain(`I/O error (Is a directory`);
        expect(result.stdout).toContain(`Run with RUST_BACKTRACE=1 to get a backtrace`);
        expect(result.stdout).not.toMatch(/^ +\d+: /m);
        expect(result.stderr).toContain(`Captured backtraces: 0\n`);
      }),
    );
  });
});
