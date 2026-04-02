import {PortablePath, xfs, npath} from '@yarnpkg/fslib';
import {yarn}                     from 'pkg-tests-core';

async function readLockfile(path: PortablePath) {
  const raw = await xfs.readFilePromise(`${path}/yarn.lock` as PortablePath, `utf8`);
  return JSON.parse(raw);
}

function getIslandNames(lockfile: any): Array<string> {
  return Object.keys(lockfile.islands ?? {});
}

function getIslandDescriptorKeys(lockfile: any, islandName: string): Array<string> {
  return Object.keys(lockfile.islands?.[islandName] ?? {});
}

function getWorkspaceHash(lockfile: any, workspaceName: string): string | undefined {
  return lockfile.workspaces?.[workspaceName];
}

describe(`Features`, () => {
  describe(`Islands`, () => {
    test(
      `it should succeed with a single island containing one workspace`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          const lockfile = await readLockfile(path);
          expect(getIslandNames(lockfile)).toEqual([`main`]);
        },
      ),
    );

    test(
      `it should succeed with multiple islands`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
          },
          [`packages/workspace-b`]: {
            name: `workspace-b`,
            version: `1.0.0`,
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              island1: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
              island2: {
                workspaces: [`workspace-b`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          const lockfile = await readLockfile(path);
          expect(getIslandNames(lockfile)).toEqual([`island1`, `island2`]);
        },
      ),
    );

    test(
      `it should still resolve non-island workspaces via greedy resolution`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
          },
          [`packages/workspace-b`]: {
            name: `workspace-b`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `2.0.0`,
            },
          },
        },
        async ({path, run, source}) => {
          // Only workspace-a is in an island; workspace-b is greedy-resolved
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          // The non-island workspace's dependency should be resolved normally
          await expect(source(`require('no-deps')`, {cwd: `${path}/packages/workspace-b` as PortablePath})).resolves.toMatchObject({
            name: `no-deps`,
            version: `2.0.0`,
          });
        },
      ),
    );

    test(
      `it should error when a workspace belongs to multiple islands`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              island1: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
              island2: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
            },
          });

          await expect(run(`install`)).rejects.toThrow(/multiple islands/i);
        },
      ),
    );

    test(
      `it should error when an island has no matching workspaces`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              empty: {
                workspaces: [`nonexistent-*`],
                linker: `node-modules`,
              },
            },
          });

          await expect(run(`install`)).rejects.toThrow(/no matching workspaces/i);
        },
      ),
    );

    test(
      `it should support glob patterns for workspace matching`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/foo`]: {
            name: `foo`,
            version: `1.0.0`,
          },
          [`packages/bar`]: {
            name: `bar`,
            version: `1.0.0`,
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              all: {
                workspaces: [`*`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          const lockfile = await readLockfile(path);
          expect(getIslandNames(lockfile)).toEqual([`all`]);
        },
      ),
    );

    test(
      `it should produce a stable lockfile across repeated installs`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          const lockfileAfterFirst = await xfs.readFilePromise(`${path}/yarn.lock` as PortablePath, `utf8`);

          await run(`install`);

          const lockfileAfterSecond = await xfs.readFilePromise(`${path}/yarn.lock` as PortablePath, `utf8`);

          expect(lockfileAfterSecond).toEqual(lockfileAfterFirst);
        },
      ),
    );

    test(
      `it should record each island separately in the lockfile`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
          },
          [`packages/workspace-b`]: {
            name: `workspace-b`,
            version: `1.0.0`,
            dependencies: {
              [`is-number`]: `1.0.0`,
            },
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              alpha: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
              beta: {
                workspaces: [`workspace-b`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          const lockfile = await readLockfile(path);
          expect(getIslandNames(lockfile)).toEqual([`alpha`, `beta`]);

          // Each island should contain only its own workspace's dependencies
          const alphaKeys = getIslandDescriptorKeys(lockfile, `alpha`);
          expect(alphaKeys.some((k: string) => k.includes(`no-deps`))).toBe(true);
          expect(alphaKeys.some((k: string) => k.includes(`is-number`))).toBe(false);

          const betaKeys = getIslandDescriptorKeys(lockfile, `beta`);
          expect(betaKeys.some((k: string) => k.includes(`is-number`))).toBe(true);
          expect(betaKeys.some((k: string) => k.includes(`no-deps`))).toBe(false);
        },
      ),
    );

    test(
      `it should keep island lockfile stable when adding a non-island workspace`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          const lockfileBefore = await readLockfile(path);
          const islandsBefore = lockfileBefore.islands;

          // Add a non-island workspace with a dependency
          const newWsPath = `${path}/packages/workspace-b` as PortablePath;
          await xfs.mkdirPromise(newWsPath, {recursive: true});
          await xfs.writeJsonPromise(`${newWsPath}/package.json` as PortablePath, {
            name: `workspace-b`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
          });

          await run(`install`);

          const lockfileAfter = await readLockfile(path);

          // The island section should be unchanged
          expect(lockfileAfter.islands).toEqual(islandsBefore);

          // The new non-island dependency should be in the main entries
          const entryKeys = Object.keys(lockfileAfter.entries);
          expect(entryKeys.some((k: string) => k.includes(`no-deps`))).toBe(true);
        },
      ),
    );

    test(
      `it should handle an island with multiple workspaces sharing the same island`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `^1.0.0`,
            },
          },
          [`packages/workspace-b`]: {
            name: `workspace-b`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `^1.0.0`,
            },
          },
          [`packages/workspace-c`]: {
            name: `workspace-c`,
            version: `1.0.0`,
            dependencies: {
              [`is-number`]: `1.0.0`,
            },
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              shared: {
                workspaces: [`workspace-a`, `workspace-b`, `workspace-c`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          // All three workspaces should have node_modules
          expect(await xfs.existsPromise(`${path}/packages/workspace-a/node_modules` as PortablePath)).toBe(true);
          expect(await xfs.existsPromise(`${path}/packages/workspace-b/node_modules` as PortablePath)).toBe(true);
          expect(await xfs.existsPromise(`${path}/packages/workspace-c/node_modules` as PortablePath)).toBe(true);

          // workspace-a and workspace-b share no-deps and should get the same version
          const versionA = await source(`require('no-deps')`, {cwd: `${path}/packages/workspace-a` as PortablePath});
          const versionB = await source(`require('no-deps')`, {cwd: `${path}/packages/workspace-b` as PortablePath});
          expect(versionA).toMatchObject({name: `no-deps`});
          expect(versionA.version).toEqual(versionB.version);

          // workspace-c resolves its own dependency
          await expect(
            source(`require('is-number')`, {cwd: `${path}/packages/workspace-c` as PortablePath}),
          ).resolves.toMatchObject({
            name: `is-number`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `it should not interfere between island and non-island resolution`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/island-ws`]: {
            name: `island-ws`,
            version: `1.0.0`,
          },
          [`packages/greedy-ws-a`]: {
            name: `greedy-ws-a`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
          },
          [`packages/greedy-ws-b`]: {
            name: `greedy-ws-b`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `2.0.0`,
            },
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              isolated: {
                workspaces: [`island-ws`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          // Both greedy workspaces should resolve their deps correctly
          await expect(source(`require('no-deps')`, {cwd: `${path}/packages/greedy-ws-a` as PortablePath})).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });

          await expect(source(`require('no-deps')`, {cwd: `${path}/packages/greedy-ws-b` as PortablePath})).resolves.toMatchObject({
            name: `no-deps`,
            version: `2.0.0`,
          });

          // The island should be recorded in the lockfile separately
          const lockfile = await readLockfile(path);
          expect(getIslandNames(lockfile)).toEqual([`isolated`]);
        },
      ),
    );

    test(
      `it should support partial glob patterns matching multiple workspaces into an island`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/app-one`]: {
            name: `app-one`,
            version: `1.0.0`,
          },
          [`packages/app-two`]: {
            name: `app-two`,
            version: `1.0.0`,
          },
          [`packages/lib-one`]: {
            name: `lib-one`,
            version: `1.0.0`,
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              apps: {
                workspaces: [`app-*`],
                linker: `node-modules`,
              },
              libs: {
                workspaces: [`lib-*`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          const lockfile = await readLockfile(path);
          expect(getIslandNames(lockfile)).toEqual([`apps`, `libs`]);
        },
      ),
    );

    test(
      `it should produce the same lockfile regardless of install order when using multiple islands`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
          },
          [`packages/workspace-b`]: {
            name: `workspace-b`,
            version: `1.0.0`,
          },
          [`packages/workspace-c`]: {
            name: `workspace-c`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              island1: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
              island2: {
                workspaces: [`workspace-b`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);
          const lockfile1 = await xfs.readFilePromise(`${path}/yarn.lock` as PortablePath, `utf8`);

          // Remove lockfile and reinstall
          await xfs.unlinkPromise(`${path}/yarn.lock` as PortablePath);
          await run(`install`);
          const lockfile2 = await xfs.readFilePromise(`${path}/yarn.lock` as PortablePath, `utf8`);

          expect(lockfile2).toEqual(lockfile1);

          // Non-island workspace should still resolve
          await expect(source(`require('no-deps')`, {cwd: `${path}/packages/workspace-c` as PortablePath})).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `it should handle a workspace moving between islands`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
          },
          [`packages/workspace-b`]: {
            name: `workspace-b`,
            version: `1.0.0`,
          },
        },
        async ({path, run}) => {
          // First install: workspace-a in island1, workspace-b in island2
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              island1: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
              island2: {
                workspaces: [`workspace-b`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          const lockfileBefore = await readLockfile(path);
          expect(getIslandNames(lockfileBefore)).toEqual([`island1`, `island2`]);

          // Move workspace-a to island2 (put both in island2, remove island1)
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              island2: {
                workspaces: [`workspace-a`, `workspace-b`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          const lockfileAfter = await readLockfile(path);
          expect(getIslandNames(lockfileAfter)).toEqual([`island2`]);
        },
      ),
    );

    test(
      `it should remove the islands section from the lockfile when all islands are removed`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          const lockfileBefore = await readLockfile(path);
          expect(getIslandNames(lockfileBefore)).toEqual([`main`]);

          // Remove all islands from configuration
          await yarn.writeConfiguration(path, {
            unstableIslands: {},
          });

          await run(`install`);

          const lockfileAfter = await readLockfile(path);
          expect(lockfileAfter.islands).toBeUndefined();
        },
      ),
    );

    test(
      `it should error when a glob pattern matches a workspace already in another island`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/app-one`]: {
            name: `app-one`,
            version: `1.0.0`,
          },
        },
        async ({path, run}) => {
          // Both globs match app-one
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              island1: {
                workspaces: [`app-*`],
                linker: `node-modules`,
              },
              island2: {
                workspaces: [`*-one`],
                linker: `node-modules`,
              },
            },
          });

          await expect(run(`install`)).rejects.toThrow(/multiple islands/i);
        },
      ),
    );

    test(
      `it should record island workspace dependencies in the island lockfile section`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          const lockfile = await readLockfile(path);
          expect(getIslandNames(lockfile)).toEqual([`main`]);

          // The island should have recorded the dependency
          const islandKeys = getIslandDescriptorKeys(lockfile, `main`);
          expect(islandKeys.some((k: string) => k.includes(`no-deps`))).toBe(true);
        },
      ),
    );

    test(
      `it should keep island dependencies separate from greedy dependencies in the lockfile`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/island-ws`]: {
            name: `island-ws`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
          },
          [`packages/greedy-ws`]: {
            name: `greedy-ws`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `2.0.0`,
            },
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`island-ws`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          const lockfile = await readLockfile(path);

          // The island section should resolve no-deps@1.0.0
          const islandEntries = lockfile.islands.main;
          const islandResolutions = Object.values(islandEntries)
            .map((entry: any) => entry.resolution.resolution)
            .filter((r: string) => r.includes(`no-deps`));
          expect(islandResolutions.some((r: string) => r.includes(`1.0.0`))).toBe(true);
          expect(islandResolutions.some((r: string) => r.includes(`2.0.0`))).toBe(false);

          // The greedy entries should resolve no-deps@2.0.0
          const greedyResolutions = Object.values(lockfile.entries)
            .map((entry: any) => entry.resolution.resolution)
            .filter((r: string) => r?.includes(`no-deps`));
          expect(greedyResolutions.some((r: string) => r.includes(`2.0.0`))).toBe(true);
        },
      ),
    );

    test(
      `it should record devDependencies in the island lockfile section`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            devDependencies: {
              [`is-number`]: `1.0.0`,
            },
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          const lockfile = await readLockfile(path);
          expect(getIslandNames(lockfile)).toEqual([`main`]);

          const islandKeys = getIslandDescriptorKeys(lockfile, `main`);
          expect(islandKeys.some((k: string) => k.includes(`is-number`))).toBe(true);
        },
      ),
    );

    test(
      `it should handle two island workspaces in the same island with different dependencies`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
          },
          [`packages/workspace-b`]: {
            name: `workspace-b`,
            version: `1.0.0`,
            dependencies: {
              [`is-number`]: `1.0.0`,
            },
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`, `workspace-b`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          const lockfile = await readLockfile(path);
          expect(getIslandNames(lockfile)).toEqual([`main`]);

          // Both dependencies should be in the island
          const islandKeys = getIslandDescriptorKeys(lockfile, `main`);
          expect(islandKeys.some((k: string) => k.includes(`no-deps`))).toBe(true);
          expect(islandKeys.some((k: string) => k.includes(`is-number`))).toBe(true);
        },
      ),
    );

    test(
      `it should keep dependencies in separate islands isolated from each other`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
          },
          [`packages/workspace-b`]: {
            name: `workspace-b`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `2.0.0`,
            },
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              island1: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
              island2: {
                workspaces: [`workspace-b`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          // Each island workspace should have its own node_modules
          expect(await xfs.existsPromise(`${path}/packages/workspace-a/node_modules` as PortablePath)).toBe(true);
          expect(await xfs.existsPromise(`${path}/packages/workspace-b/node_modules` as PortablePath)).toBe(true);

          // Each island should resolve its own version of no-deps
          await expect(
            source(`require('no-deps')`, {cwd: `${path}/packages/workspace-a` as PortablePath}),
          ).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });

          await expect(
            source(`require('no-deps')`, {cwd: `${path}/packages/workspace-b` as PortablePath}),
          ).resolves.toMatchObject({
            name: `no-deps`,
            version: `2.0.0`,
          });

          // Lockfile should show each island resolving the correct version
          const lockfile = await readLockfile(path);

          const island1Resolutions = Object.values(lockfile.islands.island1)
            .map((entry: any) => entry.resolution.resolution)
            .filter((r: string) => r.includes(`no-deps`));
          expect(island1Resolutions.some((r: string) => r.includes(`1.0.0`))).toBe(true);

          const island2Resolutions = Object.values(lockfile.islands.island2)
            .map((entry: any) => entry.resolution.resolution)
            .filter((r: string) => r.includes(`no-deps`));
          expect(island2Resolutions.some((r: string) => r.includes(`2.0.0`))).toBe(true);
        },
      ),
    );

    test(
      `it should produce a stable lockfile for islands with dependencies across repeated installs`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
          },
          [`packages/workspace-b`]: {
            name: `workspace-b`,
            version: `1.0.0`,
            dependencies: {
              [`is-number`]: `1.0.0`,
            },
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              island1: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
              island2: {
                workspaces: [`workspace-b`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);
          const lockfile1 = await xfs.readFilePromise(`${path}/yarn.lock` as PortablePath, `utf8`);

          await run(`install`);
          const lockfile2 = await xfs.readFilePromise(`${path}/yarn.lock` as PortablePath, `utf8`);

          expect(lockfile2).toEqual(lockfile1);
        },
      ),
    );

    test(
      `it should not include island workspace dependencies in the greedy resolution`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/island-ws`]: {
            name: `island-ws`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
          },
          [`packages/greedy-ws`]: {
            name: `greedy-ws`,
            version: `1.0.0`,
            dependencies: {
              [`is-number`]: `1.0.0`,
            },
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`island-ws`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          const lockfile = await readLockfile(path);

          // no-deps should be in the island section (island-ws depends on it)
          const islandKeys = getIslandDescriptorKeys(lockfile, `main`);
          expect(islandKeys.some((k: string) => k.includes(`no-deps`))).toBe(true);

          // The greedy entries should contain is-number (greedy-ws depends on it)
          // but should NOT contain no-deps as a greedy descriptor key
          const greedyDescriptorKeys = Object.keys(lockfile.entries);
          expect(greedyDescriptorKeys.some((k: string) => k.includes(`is-number`))).toBe(true);

          // no-deps should not appear as a greedy descriptor (only in the island)
          const greedyNoDepDescriptors = greedyDescriptorKeys.filter((k: string) =>
            k.includes(`no-deps`),
          );
          expect(greedyNoDepDescriptors).toHaveLength(0);
        },
      ),
    );

    test(
      `it should handle adding a dependency to an existing island workspace`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          const lockfileBefore = await readLockfile(path);
          expect(getIslandNames(lockfileBefore)).toEqual([`main`]);

          // Add a dependency to the island workspace
          await xfs.writeJsonPromise(`${path}/packages/workspace-a/package.json` as PortablePath, {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
          });

          await run(`install`);

          const lockfileAfter = await readLockfile(path);
          expect(getIslandNames(lockfileAfter)).toEqual([`main`]);

          // The newly added dependency should now appear in the island
          const islandKeys = getIslandDescriptorKeys(lockfileAfter, `main`);
          expect(islandKeys.some((k: string) => k.includes(`no-deps`))).toBe(true);
        },
      ),
    );

    test(
      `it should handle removing a dependency from an existing island workspace`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          const lockfileBefore = await readLockfile(path);
          const islandKeysBefore = getIslandDescriptorKeys(lockfileBefore, `main`);
          expect(islandKeysBefore.some((k: string) => k.includes(`no-deps`))).toBe(true);

          // Remove the dependency
          await xfs.writeJsonPromise(`${path}/packages/workspace-a/package.json` as PortablePath, {
            name: `workspace-a`,
            version: `1.0.0`,
          });

          await run(`install`);

          const lockfileAfter = await readLockfile(path);
          expect(getIslandNames(lockfileAfter)).toEqual([`main`]);

          // no-deps should no longer be in the island
          const islandKeysAfter = getIslandDescriptorKeys(lockfileAfter, `main`);
          expect(islandKeysAfter.some((k: string) => k.includes(`no-deps`))).toBe(false);
        },
      ),
    );

    test(
      `it should handle a workspace being added to an existing island via glob`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/app-one`]: {
            name: `app-one`,
            version: `1.0.0`,
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              apps: {
                workspaces: [`app-*`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          const lockfileBefore = await readLockfile(path);
          expect(getIslandNames(lockfileBefore)).toEqual([`apps`]);

          // Add a new workspace that matches the glob
          const newWsPath = `${path}/packages/app-two` as PortablePath;
          await xfs.mkdirPromise(newWsPath, {recursive: true});
          await xfs.writeJsonPromise(`${newWsPath}/package.json` as PortablePath, {
            name: `app-two`,
            version: `1.0.0`,
          });

          await run(`install`);

          const lockfileAfter = await readLockfile(path);
          expect(getIslandNames(lockfileAfter)).toEqual([`apps`]);
        },
      ),
    );

    // ---------------------------------------------------------------
    // Dependency resolution tests
    //
    // These tests exercise the island resolver's ability to resolve
    // real dependency graphs. They may fail until the resolver's
    // choose_version() and get_dependencies() TODOs are implemented.
    // ---------------------------------------------------------------

    test(
      `it should resolve transitive dependencies within an island`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              // one-fixed-dep@1.0.0 depends on no-deps@1.0.0
              [`one-fixed-dep`]: `1.0.0`,
            },
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          const lockfile = await readLockfile(path);
          const islandKeys = getIslandDescriptorKeys(lockfile, `main`);

          // Both the direct dep and the transitive dep should be recorded
          expect(islandKeys.some((k: string) => k.includes(`one-fixed-dep`))).toBe(true);
          expect(islandKeys.some((k: string) => k.includes(`no-deps`))).toBe(true);
        },
      ),
    );

    test(
      `it should resolve a range dependency to the highest matching version within an island`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              // ^1.0.0 should match 1.0.0, 1.0.1, 1.1.0 but not 2.0.0
              [`no-deps`]: `^1.0.0`,
            },
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          const lockfile = await readLockfile(path);
          const islandKeys = getIslandDescriptorKeys(lockfile, `main`);
          expect(islandKeys.some((k: string) => k.includes(`no-deps`))).toBe(true);

          // The resolved version should be in the entries and should be 1.1.0
          // (highest version matching ^1.0.0)
          const islandEntries = lockfile.islands.main;
          const noDepKey = Object.keys(islandEntries).find((k: string) => k.includes(`no-deps`));
          expect(noDepKey).toBeDefined();

          const resolution = islandEntries[noDepKey!].resolution;
          expect(resolution.resolution).toMatch(/no-deps@npm:1\.1\.0/);
        },
      ),
    );

    test(
      `it should resolve overlapping ranges to the same version within an island`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `^1.0.0`,
            },
          },
          [`packages/workspace-b`]: {
            name: `workspace-b`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `>=1.0.0 <2.0.0`,
            },
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`, `workspace-b`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          const lockfile = await readLockfile(path);
          const islandKeys = getIslandDescriptorKeys(lockfile, `main`);

          // Both ranges overlap (both include 1.x), so no-deps should resolve
          // to a single version satisfying both
          const noDepKeys = islandKeys.filter((k: string) => k.includes(`no-deps`));
          expect(noDepKeys.length).toBeGreaterThan(0);

          // There should be exactly one no-deps resolution (not two separate ones)
          const islandEntries = lockfile.islands.main;
          const noDepResolutions = Object.values(islandEntries)
            .map((entry: any) => entry.resolution.resolution)
            .filter((r: string) => r.includes(`no-deps`));

          const uniqueResolutions = [...new Set(noDepResolutions)];
          expect(uniqueResolutions).toHaveLength(1);
        },
      ),
    );

    test(
      `it should error when an island has conflicting dependency ranges that cannot be satisfied`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              // 1.0.0 and 2.0.0 are incompatible ranges
              [`no-deps`]: `1.0.0`,
            },
          },
          [`packages/workspace-b`]: {
            name: `workspace-b`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `2.0.0`,
            },
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`, `workspace-b`],
                linker: `node-modules`,
              },
            },
          });

          // Two workspaces in the same island require incompatible versions.
          // The resolver should report a conflict.
          await expect(run(`install`)).rejects.toThrow();
        },
      ),
    );

    test(
      `it should resolve a deep transitive dependency chain within an island`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              // one-range-dep@1.0.0 -> no-deps@^1.0.0
              [`one-range-dep`]: `1.0.0`,
            },
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          const lockfile = await readLockfile(path);
          const islandKeys = getIslandDescriptorKeys(lockfile, `main`);

          // Both one-range-dep and its transitive dep no-deps should be resolved
          expect(islandKeys.some((k: string) => k.includes(`one-range-dep`))).toBe(true);
          expect(islandKeys.some((k: string) => k.includes(`no-deps`))).toBe(true);
        },
      ),
    );

    test(
      `it should resolve transitive dependencies with overlapping ranges across direct and transitive deps`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              // Direct dep on no-deps@1.0.0
              [`no-deps`]: `1.0.0`,
              // one-range-dep@1.0.0 depends on no-deps@^1.0.0
              // ^1.0.0 includes 1.0.0, so these should be compatible
              [`one-range-dep`]: `1.0.0`,
            },
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          const lockfile = await readLockfile(path);
          const islandEntries = lockfile.islands.main;

          // no-deps should resolve to exactly 1.0.0 (pinned by the direct dep),
          // which also satisfies one-range-dep's ^1.0.0 requirement
          const noDepResolutions = Object.values(islandEntries)
            .map((entry: any) => entry.resolution.resolution)
            .filter((r: string) => r.includes(`no-deps`));

          const uniqueResolutions = [...new Set(noDepResolutions)];
          expect(uniqueResolutions).toHaveLength(1);
          expect(uniqueResolutions[0]).toMatch(/no-deps@npm:1\.0\.0/);
        },
      ),
    );

    test(
      `it should error when direct and transitive dependency ranges conflict within an island`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              // Direct dep on no-deps@2.0.0
              [`no-deps`]: `2.0.0`,
              // one-fixed-dep@1.0.0 depends on no-deps@1.0.0
              // 2.0.0 and 1.0.0 are incompatible
              [`one-fixed-dep`]: `1.0.0`,
            },
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
            },
          });

          // The direct dep requires 2.0.0 but the transitive dep requires 1.0.0
          await expect(run(`install`)).rejects.toThrow();
        },
      ),
    );

    test(
      `it should error when an island has both semver and non-semver references for the same package`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              // Semver reference
              [`no-deps`]: `1.0.0`,
            },
          },
          [`packages/workspace-b`]: {
            name: `workspace-b`,
            version: `1.0.0`,
            dependencies: {
              // Non-semver reference to the same package
              [`no-deps`]: `link:../local-no-deps`,
            },
          },
        },
        async ({path, run}) => {
          // Create a local package for the link reference
          const localPkgPath = `${path}/packages/local-no-deps` as PortablePath;
          await xfs.mkdirPromise(localPkgPath, {recursive: true});
          await xfs.writeJsonPromise(`${localPkgPath}/package.json` as PortablePath, {
            name: `no-deps`,
            version: `1.0.0`,
          });

          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`, `workspace-b`],
                linker: `node-modules`,
              },
            },
          });

          // One workspace wants no-deps from the registry (semver) and
          // the other wants it via link: (non-semver). The island resolver
          // uses single-version resolution, so it cannot satisfy both.
          await expect(run(`install`)).rejects.toThrow(/No solution found/i);
        },
      ),
    );

    // ---------------------------------------------------------------
    // Linking tests
    //
    // These tests verify that islands with linker: node-modules
    // produce correct node_modules trees per workspace.
    // ---------------------------------------------------------------

    test(
      `it should create node_modules for island workspaces with linker=node-modules`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          // workspace-a should have a node_modules directory
          const nmPath = `${path}/packages/workspace-a/node_modules` as PortablePath;
          expect(await xfs.existsPromise(nmPath)).toBe(true);

          // no-deps should be resolvable via node_modules
          await expect(
            source(`require('no-deps')`, {cwd: `${path}/packages/workspace-a` as PortablePath}),
          ).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `it should allow mixed PnP and node-modules island resolution`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/island-ws`]: {
            name: `island-ws`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
          },
          [`packages/pnp-ws`]: {
            name: `pnp-ws`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `2.0.0`,
            },
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`island-ws`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          // island workspace resolves via node_modules
          await expect(
            source(`require('no-deps')`, {cwd: `${path}/packages/island-ws` as PortablePath}),
          ).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });

          // PnP workspace resolves via PnP
          await expect(
            source(`require('no-deps')`, {cwd: `${path}/packages/pnp-ws` as PortablePath}),
          ).resolves.toMatchObject({
            name: `no-deps`,
            version: `2.0.0`,
          });
        },
      ),
    );

    test(
      `it should create separate node_modules for multiple workspaces in the same island`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
          },
          [`packages/workspace-b`]: {
            name: `workspace-b`,
            version: `1.0.0`,
            dependencies: {
              [`is-number`]: `1.0.0`,
            },
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`, `workspace-b`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          // Both workspaces should have node_modules
          expect(await xfs.existsPromise(`${path}/packages/workspace-a/node_modules` as PortablePath)).toBe(true);
          expect(await xfs.existsPromise(`${path}/packages/workspace-b/node_modules` as PortablePath)).toBe(true);

          // Each workspace resolves its own dependencies
          await expect(
            source(`require('no-deps')`, {cwd: `${path}/packages/workspace-a` as PortablePath}),
          ).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });

          await expect(
            source(`require('is-number')`, {cwd: `${path}/packages/workspace-b` as PortablePath}),
          ).resolves.toMatchObject({
            name: `is-number`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `it should hoist transitive dependencies into island workspace node_modules`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              // one-fixed-dep@1.0.0 depends on no-deps@1.0.0
              [`one-fixed-dep`]: `1.0.0`,
            },
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          // Both direct and transitive deps should be resolvable
          await expect(
            source(`require('one-fixed-dep')`, {cwd: `${path}/packages/workspace-a` as PortablePath}),
          ).resolves.toMatchObject({
            name: `one-fixed-dep`,
            version: `1.0.0`,
          });

          await expect(
            source(`require('no-deps')`, {cwd: `${path}/packages/workspace-a` as PortablePath}),
          ).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `it should create .bin symlinks for island workspace dependencies`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`has-bin-entries`]: `1.0.0`,
            },
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          // .bin folder should exist in the workspace's node_modules
          const binDir = npath.toPortablePath(`${path}/packages/workspace-a/node_modules/.bin`);
          await expect(xfs.lstatPromise(binDir)).resolves.toBeDefined();

          // Bin symlinks should be created for the dependency
          const binSymlink = await xfs.readlinkPromise(
            npath.toPortablePath(`${path}/packages/workspace-a/node_modules/.bin/has-bin-entries`),
          );
          expect(binSymlink).toContain(`has-bin-entries`);

          // Additional bin entries from the same package should also exist
          await expect(
            xfs.lstatPromise(npath.toPortablePath(`${path}/packages/workspace-a/node_modules/.bin/has-bin-entries-with-exit-code`)),
          ).resolves.toBeDefined();
        },
      ),
    );

    test(
      `it should handle an island workspace depending on another workspace in the same island`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`workspace-b`]: `workspace:*`,
            },
          },
          [`packages/workspace-b`]: {
            name: `workspace-b`,
            version: `1.0.0`,
            main: `./index.js`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
          },
        },
        async ({path, run, source}) => {
          // Create an index.js so require('workspace-b') has something to load
          await xfs.writeFilePromise(
            `${path}/packages/workspace-b/index.js` as PortablePath,
            `module.exports = require('./package.json');\n`,
          );

          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`, `workspace-b`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          // workspace-a should have workspace-b in its node_modules
          // (as a symlink pointing to the workspace directory)
          const wsSymlinkPath = `${path}/packages/workspace-a/node_modules/workspace-b` as PortablePath;
          expect(await xfs.existsPromise(wsSymlinkPath)).toBe(true);

          // workspace-b should resolve its own dependency
          await expect(
            source(`require('no-deps')`, {cwd: `${path}/packages/workspace-b` as PortablePath}),
          ).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });

          // workspace-a should be able to require workspace-b via node_modules
          await expect(
            source(`require('workspace-b')`, {cwd: `${path}/packages/workspace-a` as PortablePath}),
          ).resolves.toMatchObject({
            name: `workspace-b`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `it should work when project uses node-modules linker and island also uses node-modules`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/island-ws`]: {
            name: `island-ws`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
          },
          [`packages/regular-ws`]: {
            name: `regular-ws`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `2.0.0`,
            },
          },
        },
        {
          nodeLinker: `node-modules`,
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`island-ws`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          // Island workspace resolves its own version
          await expect(
            source(`require('no-deps')`, {cwd: `${path}/packages/island-ws` as PortablePath}),
          ).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });

          // Regular workspace also resolves (via project-level nm)
          await expect(
            source(`require('no-deps')`, {cwd: `${path}/packages/regular-ws` as PortablePath}),
          ).resolves.toMatchObject({
            name: `no-deps`,
            version: `2.0.0`,
          });
        },
      ),
    );

    test(
      `it should hoist shared transitive deps from multiple direct deps in island node_modules`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              // one-fixed-dep@1.0.0 depends on no-deps@1.0.0
              [`one-fixed-dep`]: `1.0.0`,
              // one-range-dep@1.0.0 depends on no-deps@^1.0.0
              // Both transitively depend on no-deps — it should be hoisted
              [`one-range-dep`]: `1.0.0`,
            },
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          // Both direct deps should resolve
          await expect(
            source(`require('one-fixed-dep')`, {cwd: `${path}/packages/workspace-a` as PortablePath}),
          ).resolves.toMatchObject({
            name: `one-fixed-dep`,
            version: `1.0.0`,
          });

          await expect(
            source(`require('one-range-dep')`, {cwd: `${path}/packages/workspace-a` as PortablePath}),
          ).resolves.toMatchObject({
            name: `one-range-dep`,
            version: `1.0.0`,
          });

          // no-deps should be hoisted to the top-level node_modules (not nested)
          const hoistedPath = `${path}/packages/workspace-a/node_modules/no-deps` as PortablePath;
          expect(await xfs.existsPromise(hoistedPath)).toBe(true);

          // Verify it's not nested under one-fixed-dep/node_modules
          const nestedPath = `${path}/packages/workspace-a/node_modules/one-fixed-dep/node_modules/no-deps` as PortablePath;
          expect(await xfs.existsPromise(nestedPath)).toBe(false);
        },
      ),
    );

    test(
      `it should resolve workspace and semver dependencies together inside an island`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
              [`workspace-b`]: `workspace:*`,
            },
          },
          [`packages/workspace-b`]: {
            name: `workspace-b`,
            version: `1.0.0`,
            main: `./index.js`,
          },
        },
        async ({path, run, source}) => {
          await xfs.writeFilePromise(
            `${path}/packages/workspace-b/index.js` as PortablePath,
            `module.exports = require('./package.json');\n`,
          );

          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          await expect(
            source(`require('no-deps')`, {cwd: `${path}/packages/workspace-a` as PortablePath}),
          ).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });

          await expect(
            source(`require('workspace-b')`, {cwd: `${path}/packages/workspace-a` as PortablePath}),
          ).resolves.toMatchObject({
            name: `workspace-b`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `it should update island workspace tree hashes when island dependencies change`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);
          const lockfileBefore = await readLockfile(path);
          const hashBefore = getWorkspaceHash(lockfileBefore, `workspace-a`);
          expect(hashBefore).toBeDefined();

          await xfs.writeJsonPromise(`${path}/packages/workspace-a/package.json` as PortablePath, {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `2.0.0`,
            },
          });

          await run(`install`);

          const lockfileAfter = await readLockfile(path);
          const hashAfter = getWorkspaceHash(lockfileAfter, `workspace-a`);
          expect(hashAfter).toBeDefined();
          expect(hashAfter).not.toEqual(hashBefore);
        },
      ),
    );

    test(
      `it should install devDependencies in island workspace node_modules`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
            devDependencies: {
              [`is-number`]: `1.0.0`,
            },
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);

          // Regular dependency should be available
          await expect(
            source(`require('no-deps')`, {cwd: `${path}/packages/workspace-a` as PortablePath}),
          ).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });

          // devDependency should also be available in node_modules
          await expect(
            source(`require('is-number')`, {cwd: `${path}/packages/workspace-a` as PortablePath}),
          ).resolves.toMatchObject({
            name: `is-number`,
            version: `1.0.0`,
          });
        },
      ),
    );
  });
});
