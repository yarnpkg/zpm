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

    test(
      `it should keep unchanged store folders across installs`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
          [`one-fixed-dep`]: `1.0.0`,
        },
      }, {
        nodeLinker: `pnpm`,
      }, async ({path, run}) => {
        await run(`install`);

        const noDepsPath = ppath.join(path, `node_modules/no-deps/package.json`);
        const marker = ppath.join(ppath.dirname(await xfs.realpathPromise(noDepsPath)), `marker`);

        // A file Yarn doesn't know about survives only if the folder isn't re-extracted
        await xfs.writeFilePromise(marker, ``);

        // Changing an unrelated dependency relinks the project
        await xfs.writeJsonPromise(ppath.join(path, `package.json`), {
          dependencies: {
            [`no-deps`]: `1.0.0`,
            [`one-fixed-dep`]: `2.0.0`,
          },
        });

        await run(`install`);
        expect(xfs.existsSync(marker)).toEqual(true);

        // --force is the escape hatch to heal a damaged store
        await run(`install`, `--force`);
        expect(xfs.existsSync(marker)).toEqual(false);
      }),
    );

    test(
      `it should re-extract store folders whose patch changed`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `patch:no-deps@npm%3A1.0.0#./my.patch`,
        },
      }, {
        nodeLinker: `pnpm`,
      }, async ({path, run}) => {
        const makePatch = (value: string) => [
          `diff --git a/index.js b/index.js`,
          `--- a/index.js`,
          `+++ b/index.js`,
          `@@ -1,0 +1,1 @@`,
          `+module.exports.patched = ${JSON.stringify(value)};`,
          ``,
        ].join(`\n`);

        await xfs.writeFilePromise(ppath.join(path, `my.patch`), makePatch(`first`));
        await run(`install`);

        await xfs.writeFilePromise(ppath.join(path, `my.patch`), makePatch(`second`));
        await run(`install`);

        const content = await xfs.readFilePromise(ppath.join(path, `node_modules/no-deps/index.js`), `utf8`);
        expect(content).toContain(`"second"`);
        expect(content).not.toContain(`"first"`);
      }),
    );

    test(
      `it should remove links to dependencies that were removed`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
          [`@types/no-deps`]: `1.0.0`,
        },
      }, {
        nodeLinker: `pnpm`,
      }, async ({path, run}) => {
        await run(`install`);

        expect(xfs.existsSync(ppath.join(path, `node_modules/no-deps`))).toEqual(true);
        expect(xfs.existsSync(ppath.join(path, `node_modules/@types/no-deps`))).toEqual(true);

        await xfs.writeJsonPromise(ppath.join(path, `package.json`), {dependencies: {}});
        await run(`install`);

        expect(xfs.existsSync(ppath.join(path, `node_modules/no-deps`))).toEqual(false);
        expect(xfs.existsSync(ppath.join(path, `node_modules/@types/no-deps`))).toEqual(false);
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
