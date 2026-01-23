import {WindowsLinkType}                 from '@yarnpkg/core';
import {PortablePath, ppath, npath, xfs} from '@yarnpkg/fslib';

const {
  fs: {FsLinkType, determineLinkType},
  tests: {testIf},
} = require(`pkg-tests-core`);

describe(`Features`, () => {
  describe(`Pnpm Mode `, () => {
    test(
      `it shouldn't crash if we recursively traverse a node_modules`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, {
        nodeLinker: `pnpm`,
      }, async ({path, run, source}) => {
        await run(`install`);

        let iterationCount = 0;

        const getRecursiveDirectoryListing = async (p: PortablePath) => {
          if (iterationCount++ > 500)
            throw new Error(`Possible infinite recursion detected`);

          for (const entry of await xfs.readdirPromise(p)) {
            const entryPath = ppath.join(p, entry);
            const stat = await xfs.statPromise(entryPath);

            if (stat.isDirectory()) {
              await getRecursiveDirectoryListing(entryPath);
            }
          }
        };

        await getRecursiveDirectoryListing(path);
      }),
    );

    testIf(() => process.platform === `win32`,
      `'winLinkType: symlinks' on Windows should use symlinks in node_modules directories`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`no-deps`]: `1.0.0`,
          },
        },
        {
          nodeLinker: `pnpm`,
          winLinkType: WindowsLinkType.SYMLINKS,
        },
        async ({path, run}) => {
          await run(`install`);

          const packageLinkPath = npath.toPortablePath(`${path}/node_modules/no-deps`);
          expect(await determineLinkType(packageLinkPath)).toEqual(FsLinkType.SYMBOLIC);
          expect(ppath.isAbsolute(await xfs.readlinkPromise(npath.toPortablePath(packageLinkPath)))).toBeFalsy();
        },
      ),
    );

    testIf(() => process.platform === `win32`,
      `'winLinkType: junctions' on Windows should use junctions in node_modules directories`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`no-deps`]: `1.0.0`,
          },
        },
        {
          nodeLinker: `pnpm`,
          winLinkType: WindowsLinkType.JUNCTIONS,
        },
        async ({path, run}) => {
          await run(`install`);
          const packageLinkPath = npath.toPortablePath(`${path}/node_modules/no-deps`);
          expect(await determineLinkType(packageLinkPath)).toEqual(FsLinkType.NTFS_JUNCTION);
          expect(ppath.isAbsolute(await xfs.readlinkPromise(packageLinkPath))).toBeTruthy();
        },
      ),
    );

    testIf(() => process.platform !== `win32`,
      `'winLinkType: junctions' not-on Windows should use symlinks in node_modules directories`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`no-deps`]: `1.0.0`,
          },
        },
        {
          nodeLinker: `pnpm`,
          winLinkType: WindowsLinkType.JUNCTIONS,
        },
        async ({path, run}) => {
          await run(`install`);
          const packageLinkPath = npath.toPortablePath(`${path}/node_modules/no-deps`);
          const packageLinkStat = await xfs.lstatPromise(packageLinkPath);

          expect(ppath.isAbsolute(await xfs.readlinkPromise(packageLinkPath))).toBeFalsy();
          expect(packageLinkStat.isSymbolicLink()).toBeTruthy();
        },
      ),
    );

    testIf(() => process.platform !== `win32`,
      `'winLinkType: symlinks' not-on Windows should use symlinks in node_modules directories`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`no-deps`]: `1.0.0`,
          },
        },
        {
          nodeLinker: `pnpm`,
          winLinkType: WindowsLinkType.SYMLINKS,
        },
        async ({path, run}) => {
          await run(`install`);

          const packageLinkPath = npath.toPortablePath(`${path}/node_modules/no-deps`);
          const packageLinkStat = await xfs.lstatPromise(packageLinkPath);

          expect(ppath.isAbsolute(await xfs.readlinkPromise(packageLinkPath))).toBeFalsy();
          expect(packageLinkStat.isSymbolicLink()).toBeTruthy();
        },
      ),
    );

    test(
      `pnpmHoistPatterns should hoist matching packages to store node_modules`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`one-fixed-dep`]: `1.0.0`,
          },
        },
        {
          nodeLinker: `pnpm`,
          pnpmHoistPatterns: [`*`],
        },
        async ({path, run}) => {
          await run(`install`);

          // The transitive dependency 'no-deps' should be hoisted to the store's node_modules
          const hoistedPath = npath.toPortablePath(`${path}/node_modules/.pnpm/node_modules/no-deps`);
          const hoistedStat = await xfs.lstatPromise(hoistedPath);
          expect(hoistedStat.isSymbolicLink()).toBeTruthy();
        },
      ),
    );

    test(
      `pnpmHoistPatterns with empty array should disable hoisting to store`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`one-fixed-dep`]: `1.0.0`,
          },
        },
        {
          nodeLinker: `pnpm`,
          pnpmHoistPatterns: [],
        },
        async ({path, run}) => {
          await run(`install`);

          // The store's shared node_modules should not exist or be empty
          const storeNmPath = npath.toPortablePath(`${path}/node_modules/.pnpm/node_modules`);
          await expect(xfs.existsPromise(storeNmPath)).resolves.toBeFalsy();
        },
      ),
    );

    test(
      `pnpmHoistPatterns should only hoist packages matching the pattern`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`one-fixed-dep`]: `1.0.0`,
            [`no-deps`]: `2.0.0`,
          },
        },
        {
          nodeLinker: `pnpm`,
          pnpmHoistPatterns: [`one-*`],
        },
        async ({path, run}) => {
          await run(`install`);

          // one-fixed-dep should be hoisted
          const hoistedPath = npath.toPortablePath(`${path}/node_modules/.pnpm/node_modules/one-fixed-dep`);
          const hoistedStat = await xfs.lstatPromise(hoistedPath);
          expect(hoistedStat.isSymbolicLink()).toBeTruthy();

          // no-deps should NOT be hoisted (doesn't match pattern)
          const notHoistedPath = npath.toPortablePath(`${path}/node_modules/.pnpm/node_modules/no-deps`);
          await expect(xfs.existsPromise(notHoistedPath)).resolves.toBeFalsy();
        },
      ),
    );

    test(
      `pnpmPublicHoistPatterns should hoist matching transitive dependencies to root node_modules`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`one-fixed-dep`]: `1.0.0`,
          },
        },
        {
          nodeLinker: `pnpm`,
          pnpmPublicHoistPatterns: [`no-deps`],
        },
        async ({path, run}) => {
          await run(`install`);

          // no-deps is a transitive dependency of one-fixed-dep
          // With public hoisting, it should appear in root node_modules
          const publicHoistedPath = npath.toPortablePath(`${path}/node_modules/no-deps`);
          const publicHoistedStat = await xfs.lstatPromise(publicHoistedPath);
          expect(publicHoistedStat.isSymbolicLink()).toBeTruthy();
        },
      ),
    );

    test(
      `pnpmPublicHoistPatterns should not override direct dependencies`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`one-fixed-dep`]: `1.0.0`,
            [`no-deps`]: `2.0.0`,
          },
        },
        {
          nodeLinker: `pnpm`,
          pnpmPublicHoistPatterns: [`no-deps`],
        },
        async ({path, run, source}) => {
          await run(`install`);

          // no-deps@2.0.0 is a direct dependency, so it should be in root node_modules
          // The public hoist pattern should not override it with the transitive no-deps@1.0.0
          await expect(source(`require('no-deps/package.json').version`)).resolves.toEqual(`2.0.0`);
        },
      ),
    );

    test(
      `pnpmPublicHoistPatterns with wildcard should hoist all transitive dependencies`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`one-fixed-dep`]: `1.0.0`,
          },
        },
        {
          nodeLinker: `pnpm`,
          pnpmPublicHoistPatterns: [`*`],
        },
        async ({path, run, source}) => {
          await run(`install`);

          // With wildcard public hoisting, transitive dependencies should be accessible from root
          await expect(source(`require('no-deps/package.json').name`)).resolves.toEqual(`no-deps`);
        },
      ),
    );
  });
});
