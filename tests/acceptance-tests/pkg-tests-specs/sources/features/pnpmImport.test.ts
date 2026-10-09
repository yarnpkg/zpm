import {PortablePath, ppath, xfs} from '@yarnpkg/fslib';
import {yarn}                     from 'pkg-tests-core';

type Importer = Record<string, Record<string, {specifier: string, version: string}>>;

type PnpmLockfile = {
  catalogs?: Record<string, Record<string, {specifier: string, version: string}>>;
  importers?: Record<string, Importer>;
  snapshots?: Record<string, {dependencies?: Record<string, string>}>;
};

async function writePnpmLockfile(path: PortablePath, lockfile: PnpmLockfile) {
  // YAML is a superset of JSON, which is good enough for our purposes
  await xfs.writeFilePromise(ppath.join(path, `pnpm-lock.yaml`), JSON.stringify({
    lockfileVersion: `9.0`,
    settings: {autoInstallPeers: true, excludeLinksFromLockfile: false},
    ...lockfile,
    packages: Object.fromEntries(Object.keys(lockfile.snapshots ?? {}).map(key => [key.replace(/\(.*/, ``), {}])),
  }, null, 2));
}

describe(`Features`, () => {
  describe(`pnpm import`, () => {
    test(
      `it should keep the versions locked by pnpm for the workspace dependencies`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `^1.0.0`,
        },
      }, async ({path, run, source}) => {
        await writePnpmLockfile(path, {
          importers: {
            [`.`]: {dependencies: {[`no-deps`]: {specifier: `^1.0.0`, version: `1.0.0`}}},
          },
          snapshots: {
            [`no-deps@1.0.0`]: {},
          },
        });

        const {stdout} = await run(`install`);
        expect(stdout).toContain(`Yarn will prefer the`);

        await expect(source(`require('no-deps')`)).resolves.toMatchObject({
          version: `1.0.0`,
        });
      }),
    );

    test(
      `it should keep the versions locked by pnpm for dist-tags`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `latest`,
        },
      }, async ({path, run, source}) => {
        // `latest` points to 2.0.0 now, but pnpm locked it at 1.0.1
        await writePnpmLockfile(path, {
          importers: {
            [`.`]: {dependencies: {[`no-deps`]: {specifier: `latest`, version: `1.0.1`}}},
          },
          snapshots: {
            [`no-deps@1.0.1`]: {},
          },
        });

        await run(`install`);

        await expect(source(`require('no-deps')`)).resolves.toMatchObject({
          version: `1.0.1`,
        });
      }),
    );

    test(
      `it should keep the versions locked by pnpm for transitive dependencies`,
      makeTemporaryEnv({
        dependencies: {
          [`one-range-dep`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await writePnpmLockfile(path, {
          importers: {
            [`.`]: {dependencies: {[`one-range-dep`]: {specifier: `1.0.0`, version: `1.0.0`}}},
          },
          snapshots: {
            [`one-range-dep@1.0.0`]: {dependencies: {[`no-deps`]: `1.0.1`}},
            [`no-deps@1.0.1`]: {},
          },
        });

        await run(`install`);

        await expect(source(`require('one-range-dep')`)).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: {
              version: `1.0.1`,
            },
          },
        });
      }),
    );

    test(
      `it should pick the version locked for the range when several locked versions satisfy it`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `^1.0.0`,
          [`one-range-dep`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await writePnpmLockfile(path, {
          importers: {
            [`.`]: {dependencies: {
              [`no-deps`]: {specifier: `^1.0.0`, version: `1.0.0`},
              [`one-range-dep`]: {specifier: `1.0.0`, version: `1.0.0`},
            }},
          },
          snapshots: {
            // Both 1.0.0 and 1.1.0 satisfy ^1.0.0; the highest would be
            // picked if we didn't check which range each one got locked for
            [`one-range-dep@1.0.0`]: {dependencies: {[`no-deps`]: `1.0.0`}},
            [`no-deps@1.0.0`]: {},
            [`no-deps@1.1.0`]: {},
          },
        });

        await run(`install`);

        await expect(source(`require('no-deps')`)).resolves.toMatchObject({
          version: `1.0.0`,
        });

        await expect(source(`require('one-range-dep')`)).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: {
              version: `1.0.0`,
            },
          },
        });
      }),
    );

    test(
      `it should support catalogs`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `catalog:`,
        },
      }, async ({path, run, source}) => {
        await yarn.writeConfiguration(path, {
          catalog: {
            [`no-deps`]: `^1.0.0`,
          },
        });

        await writePnpmLockfile(path, {
          catalogs: {
            default: {[`no-deps`]: {specifier: `^1.0.0`, version: `1.0.1`}},
          },
          importers: {
            [`.`]: {dependencies: {[`no-deps`]: {specifier: `catalog:`, version: `1.0.1`}}},
          },
          snapshots: {
            [`no-deps@1.0.1`]: {},
          },
        });

        await run(`install`);

        await expect(source(`require('no-deps')`)).resolves.toMatchObject({
          version: `1.0.1`,
        });
      }),
    );

    test(
      `it should support aliases`,
      makeTemporaryEnv({
        dependencies: {
          [`my-alias`]: `npm:no-deps@^1.0.0`,
        },
      }, async ({path, run, source}) => {
        await writePnpmLockfile(path, {
          importers: {
            [`.`]: {dependencies: {[`my-alias`]: {specifier: `npm:no-deps@^1.0.0`, version: `no-deps@1.0.0`}}},
          },
          snapshots: {
            [`no-deps@1.0.0`]: {},
          },
        });

        await run(`install`);

        await expect(source(`require('my-alias')`)).resolves.toMatchObject({
          name: `no-deps`,
          version: `1.0.0`,
        });
      }),
    );

    test(
      `it should fallback to a regular resolution when the range doesn't match the locked version anymore`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `^2.0.0`,
          [`one-range-dep`]: `1.0.0`,
        },
        resolutions: {
          [`one-range-dep/no-deps`]: `1.0.1`,
        },
      }, async ({path, run, source}) => {
        await writePnpmLockfile(path, {
          importers: {
            [`.`]: {dependencies: {
              [`no-deps`]: {specifier: `^1.0.0`, version: `1.0.0`},
              [`one-range-dep`]: {specifier: `1.0.0`, version: `1.0.0`},
            }},
          },
          snapshots: {
            [`one-range-dep@1.0.0`]: {dependencies: {[`no-deps`]: `1.0.0`}},
            [`no-deps@1.0.0`]: {},
          },
        });

        await run(`install`);

        await expect(source(`require('no-deps')`)).resolves.toMatchObject({
          version: `2.0.0`,
        });

        await expect(source(`require('one-range-dep')`)).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: {
              version: `1.0.1`,
            },
          },
        });
      }),
    );

    test(
      `it should exempt the versions locked by pnpm from the minimal age gate`,
      makeTemporaryEnv({
        dependencies: {
          [`release-date`]: `^1.0.0`,
        },
      }, {
        npmMinimalAgeGate: `1d`,
      }, async ({path, run, source}) => {
        await writePnpmLockfile(path, {
          importers: {
            [`.`]: {dependencies: {[`release-date`]: {specifier: `^1.0.0`, version: `1.1.1`}}},
          },
          snapshots: {
            [`release-date@1.1.1`]: {dependencies: {[`release-date-transitive`]: `1.1.1`}},
            [`release-date-transitive@1.1.1`]: {},
          },
        });

        await run(`install`);

        await expect(source(`require('release-date')`)).resolves.toMatchObject({
          version: `1.1.1`,
          dependencies: {
            [`release-date-transitive`]: {
              version: `1.1.1`,
            },
          },
        });
      }),
    );

    test(
      `it should leave the existing yarn.lock alone, unless explicitly asked to import`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `^1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        await expect(source(`require('no-deps')`)).resolves.toMatchObject({
          version: `1.1.0`,
        });

        await writePnpmLockfile(path, {
          importers: {
            [`.`]: {dependencies: {[`no-deps`]: {specifier: `^1.0.0`, version: `1.0.0`}}},
          },
          snapshots: {
            [`no-deps@1.0.0`]: {},
          },
        });

        await run(`install`);

        await expect(source(`require('no-deps')`)).resolves.toMatchObject({
          version: `1.1.0`,
        });

        await run(`import`, `pnpm`);

        await expect(source(`require('no-deps')`)).resolves.toMatchObject({
          version: `1.0.0`,
        });
      }),
    );

    test(
      `it should support workspaces`,
      makeTemporaryMonorepoEnv({
        workspaces: [`packages/*`],
      }, {
        [`packages/a`]: {
          name: `a`,
          dependencies: {
            [`b`]: `workspace:*`,
            [`no-deps`]: `^1.0.0`,
          },
        },
        [`packages/b`]: {
          name: `b`,
          dependencies: {
            [`no-deps`]: `^2.0.0 || ^1.0.0`,
          },
        },
      }, async ({path, run}) => {
        await writePnpmLockfile(path, {
          importers: {
            [`packages/a`]: {dependencies: {
              [`b`]: {specifier: `workspace:*`, version: `link:../b`},
              [`no-deps`]: {specifier: `^1.0.0`, version: `1.0.0`},
            }},
            [`packages/b`]: {dependencies: {
              [`no-deps`]: {specifier: `^2.0.0 || ^1.0.0`, version: `1.0.1`},
            }},
          },
          snapshots: {
            [`no-deps@1.0.0`]: {},
            [`no-deps@1.0.1`]: {},
          },
        });

        await run(`install`);

        const lockfile = await xfs.readJsonPromise(ppath.join(path, `yarn.lock`));
        expect(lockfile.entries[`no-deps@npm:^1.0.0`].resolution.version).toEqual(`1.0.0`);
        expect(lockfile.entries[`no-deps@npm:^2.0.0 || ^1.0.0`].resolution.version).toEqual(`1.0.1`);
      }),
    );
  });
});
