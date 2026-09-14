import {LOCKFILE_VERSION}     from '@yarnpkg/core';
import {Filename, ppath, xfs} from '@yarnpkg/fslib';
import {yarn}                 from 'pkg-tests-core';

const {
  tests: {setPackageWhitelist},
} = require(`pkg-tests-core`);

describe(`Lock tests`, () => {
  for (const mode of [null, `update-lockfile`]) {
    const args = mode !== null ? [`--mode=${mode}`] : [];

    describe(`Workspace checksums (${mode ?? `default`})`, () => {
      test(
        `it should store workspace checksums by default`,
        makeTemporaryEnv({
          name: `root`,
          dependencies: {[`no-deps`]: `1.0.0`},
        }, async ({path, run}) => {
          await run(`install`, ...args);

          await expect(xfs.readJsonPromise(ppath.join(path, Filename.lockfile))).resolves.toMatchObject({
            workspaces: {root: expect.any(String)},
          });
        }),
      );

      test(
        `it should omit workspace checksums when disabled`,
        makeTemporaryEnv({
          name: `root`,
          dependencies: {[`no-deps`]: `1.0.0`},
        }, async ({path, run}) => {
          await yarn.writeConfiguration(path, {enableWorkspaceChecksums: false});
          await run(`install`, ...args);

          const lockfile = await xfs.readJsonPromise(ppath.join(path, Filename.lockfile));
          expect(lockfile).not.toHaveProperty(`workspaces`);
          expect(lockfile.entries).toHaveProperty([`no-deps@npm:1.0.0`]);

          await run(`install`, `--immutable`);
        }),
      );

      test(
        `it should remove existing workspace checksums and restore them when re-enabled`,
        makeTemporaryEnv({
          name: `root`,
          dependencies: {[`no-deps`]: `1.0.0`},
        }, async ({path, run}) => {
          await run(`install`, ...args);

          const lockfilePath = ppath.join(path, Filename.lockfile);
          const originalLockfile = await xfs.readJsonPromise(lockfilePath);

          await yarn.writeConfiguration(path, {enableWorkspaceChecksums: false});
          await run(`install`, ...args);

          const lockfile = await xfs.readJsonPromise(lockfilePath);
          expect(lockfile).not.toHaveProperty(`workspaces`);
          expect(lockfile.entries).toEqual(originalLockfile.entries);

          await yarn.writeConfiguration(path, {enableWorkspaceChecksums: true});
          await run(`install`, ...args);

          await expect(xfs.readJsonPromise(lockfilePath)).resolves.toEqual(originalLockfile);
        }),
      );
    });
  }

  test(
    `it should correctly lock dependencies`,
    makeTemporaryEnv(
      {
        dependencies: {[`no-deps`]: `^1.0.0`},
      },
      async ({path, run, source}) => {
        await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
          await run(`install`);
        });
        await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`, `1.1.0`])]]), async () => {
          await run(`install`);
        });
        await expect(source(`require('no-deps')`)).resolves.toMatchObject({
          name: `no-deps`,
          version: `1.0.0`,
        });
      },
    ),
  );

  test(
    `it shouldn't loose track of the resolutions when upgrading the lockfile version`,
    makeTemporaryEnv(
      {
        dependencies: {[`one-range-dep`]: `1.0.0`},
      },
      async ({path, run, source}) => {
        await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
          await run(`install`);
        });

        await expect(source(`require('one-range-dep')`)).resolves.toMatchObject({
          name: `one-range-dep`,
          version: `1.0.0`,
          dependencies: {
            [`no-deps`]: {
              name: `no-deps`,
              version: `1.0.0`,
            },
          },
        });

        await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`, `1.1.0`])]]), async () => {
          await run(`install`, {
            lockfileVersionOverride: LOCKFILE_VERSION + 1,
          });
        });

        await expect(source(`require('one-range-dep')`)).resolves.toMatchObject({
          name: `one-range-dep`,
          version: `1.0.0`,
          dependencies: {
            [`no-deps`]: {
              name: `no-deps`,
              version: `1.0.0`,
            },
          },
        });

        await xfs.rmPromise(ppath.join(path, Filename.lockfile));

        await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`, `1.1.0`])]]), async () => {
          await run(`install`, {
            lockfileVersionOverride: LOCKFILE_VERSION + 2,
          });
        });

        await expect(source(`require('one-range-dep')`)).resolves.toMatchObject({
          name: `one-range-dep`,
          version: `1.0.0`,
          dependencies: {
            [`no-deps`]: {
              name: `no-deps`,
              version: `1.1.0`,
            },
          },
        });
      },
    ),
  );
});
