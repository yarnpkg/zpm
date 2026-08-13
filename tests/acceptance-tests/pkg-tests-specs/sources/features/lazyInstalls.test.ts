import {Filename, PortablePath, ppath, xfs} from '@yarnpkg/fslib';
import {yarn}                               from 'pkg-tests-core';

async function getCacheContent(cacheFolder: PortablePath) {
  const cacheContent = (await xfs.readdirPromise(cacheFolder))
    .filter(file => !file.startsWith(`.`));

  cacheContent.sort();

  return cacheContent;
}

async function setupMonorepo(path: PortablePath, configureFocusedMode = true) {
  if (configureFocusedMode) {
    await yarn.writeConfiguration(path, {
      lazyInstallMode: `focused`,
    });
  }

  const pkg = async (name: string, manifest: Record<string, any>) => {
    await xfs.mkdirpPromise(ppath.join(path, `packages/${name}` as PortablePath));
    await xfs.writeJsonPromise(ppath.join(path, `packages/${name}/package.json` as PortablePath), {name, ...manifest});
  };

  await xfs.writeJsonPromise(ppath.join(path, Filename.manifest), {
    private: true,
    workspaces: [`packages/*`],
  });

  await pkg(`foo`, {
    dependencies: {
      [`no-deps`]: `1.0.0`,
    },
  });

  await pkg(`bar`, {
    dependencies: {
      [`no-deps`]: `2.0.0`,
    },
  });

  await pkg(`baz`, {
    dependencies: {
      [`one-fixed-dep`]: `1.0.0`,
    },
  });
}

describe(`Features`, () => {
  describe(`Lazy installs`, () => {
    test(
      `it should not run install when running a command twice in a row`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        const installStatePath = ppath.join(path, `.yarn/ignore/install` as PortablePath);
        const stateBefore = await xfs.statPromise(installStatePath);

        // Wait a tiny bit to ensure different timestamps would be detectable
        await new Promise(resolve => setTimeout(resolve, 10));

        await run(`node`, `-e`, `console.log('hello')`);

        const stateAfter = await xfs.statPromise(installStatePath);
        expect(stateAfter.mtimeMs).toEqual(stateBefore.mtimeMs);
      }),
    );

    test(
      `it should run install when package.json is modified`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        const installStatePath = ppath.join(path, `.yarn/ignore/install` as PortablePath);
        const stateBefore = await xfs.statPromise(installStatePath);

        // Wait a tiny bit to ensure different timestamps
        await new Promise(resolve => setTimeout(resolve, 10));

        const manifestPath = ppath.join(path, Filename.manifest);
        const manifest = await xfs.readJsonPromise(manifestPath);
        manifest.dependencies[`one-fixed-dep`] = `1.0.0`;
        await xfs.writeJsonPromise(manifestPath, manifest);

        await run(`node`, `-e`, `console.log('hello')`);

        const stateAfter = await xfs.statPromise(installStatePath);
        expect(stateAfter.mtimeMs).toBeGreaterThan(stateBefore.mtimeMs);

        await expect(source(`require('one-fixed-dep')`)).resolves.toMatchObject({
          name: `one-fixed-dep`,
          version: `1.0.0`,
        });
      }),
    );

    test(
      `it should run install when project configuration is modified`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        const installStatePath = ppath.join(path, `.yarn/ignore/install` as PortablePath);
        const stateBefore = await xfs.statPromise(installStatePath);

        await new Promise(resolve => setTimeout(resolve, 10));

        await yarn.writeConfiguration(path, {
          preferInteractive: true,
        });

        await run(`node`, `-e`, `console.log('hello')`);

        const stateAfter = await xfs.statPromise(installStatePath);
        expect(stateAfter.mtimeMs).toBeGreaterThan(stateBefore.mtimeMs);
      }),
    );

    test(
      `it should run install when user configuration is modified`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        const installStatePath = ppath.join(path, `.yarn/ignore/install` as PortablePath);
        const stateBefore = await xfs.statPromise(installStatePath);

        // Wait a tiny bit to ensure different timestamps
        await new Promise(resolve => setTimeout(resolve, 10));

        const userConfigPath = ppath.join(path, `..` as PortablePath);
        await yarn.writeConfiguration(userConfigPath, {
          preferInteractive: true,
        });

        await run(`node`, `-e`, `console.log('hello')`);

        const stateAfter = await xfs.statPromise(installStatePath);
        expect(stateAfter.mtimeMs).toBeGreaterThan(stateBefore.mtimeMs);
      }),
    );

    test(
      `it should not run install when the active workspace is covered by a focused install`,
      makeTemporaryEnv({}, async ({path, run}) => {
        await setupMonorepo(path);
        await run(`install`);
        await run(`workspaces`, `focus`, `foo`, {cwd: ppath.join(path, `packages/foo` as PortablePath)});

        const installStatePath = ppath.join(path, `.yarn/ignore/install` as PortablePath);
        const stateBefore = await xfs.statPromise(installStatePath);

        await new Promise(resolve => setTimeout(resolve, 10));

        await run(`node`, `-e`, `require('no-deps')`, {
          cwd: ppath.join(path, `packages/foo` as PortablePath),
        });

        const stateAfter = await xfs.statPromise(installStatePath);
        expect(stateAfter.mtimeMs).toEqual(stateBefore.mtimeMs);
      }),
    );

    test(
      `it should extend a focused install when the active workspace is missing and the lockfile is fresh`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await setupMonorepo(path);
        await run(`install`);

        const cacheFolder = ppath.join(path, `.yarn/cache` as PortablePath);
        await xfs.removePromise(cacheFolder);

        await run(`workspaces`, `focus`, `foo`, {cwd: ppath.join(path, `packages/foo` as PortablePath)});

        await run(`node`, `-e`, `require('no-deps')`, {
          cwd: ppath.join(path, `packages/bar` as PortablePath),
        });

        await expect(source(`require('no-deps')`, {
          cwd: ppath.join(path, `packages/foo` as PortablePath),
        })).resolves.toMatchObject({
          name: `no-deps`,
          version: `1.0.0`,
        });

        await expect(source(`require('no-deps')`, {
          cwd: ppath.join(path, `packages/bar` as PortablePath),
        })).resolves.toMatchObject({
          name: `no-deps`,
          version: `2.0.0`,
        });

        await expect(getCacheContent(cacheFolder)).resolves.toEqual([
          expect.stringContaining(`no-deps-npm-1.0.0-`),
          expect.stringContaining(`no-deps-npm-2.0.0-`),
        ]);
      }),
    );

    test(
      `it should run a full lazy install by default`,
      makeTemporaryEnv({}, async ({path, run}) => {
        await setupMonorepo(path, false);

        await run(`install`);

        const cacheFolder = ppath.join(path, `.yarn/cache` as PortablePath);
        await xfs.removePromise(cacheFolder);

        await run(`workspaces`, `focus`, `foo`, {cwd: ppath.join(path, `packages/foo` as PortablePath)});

        await run(`node`, `-e`, `require('no-deps')`, {
          cwd: ppath.join(path, `packages/bar` as PortablePath),
        });

        await expect(getCacheContent(cacheFolder)).resolves.toEqual([
          expect.stringContaining(`no-deps-npm-1.0.0-`),
          expect.stringContaining(`no-deps-npm-2.0.0-`),
          expect.stringContaining(`one-fixed-dep-npm-1.0.0-`),
        ]);
      }),
    );

    test(
      `it should fall back to a full lazy install when the project configuration changes`,
      makeTemporaryEnv({}, async ({path, run}) => {
        await setupMonorepo(path);
        await run(`install`);

        const cacheFolder = ppath.join(path, `.yarn/cache` as PortablePath);
        await xfs.removePromise(cacheFolder);

        await run(`workspaces`, `focus`, `foo`, {cwd: ppath.join(path, `packages/foo` as PortablePath)});

        await yarn.writeConfiguration(path, {
          lazyInstallMode: `focused`,
          preferInteractive: true,
        });

        await run(`node`, `-e`, `require('no-deps')`, {
          cwd: ppath.join(path, `packages/bar` as PortablePath),
        });

        await expect(getCacheContent(cacheFolder)).resolves.toEqual([
          expect.stringContaining(`no-deps-npm-1.0.0-`),
          expect.stringContaining(`no-deps-npm-2.0.0-`),
          expect.stringContaining(`one-fixed-dep-npm-1.0.0-`),
        ]);
      }),
    );

    test(
      `it should treat transitive workspace dependencies as covered by focused installs`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await xfs.writeJsonPromise(ppath.join(path, Filename.manifest), {
          private: true,
          workspaces: [`packages/*`],
        });

        await xfs.mkdirpPromise(ppath.join(path, `packages/app` as PortablePath));
        await xfs.writeJsonPromise(ppath.join(path, `packages/app/package.json` as PortablePath), {
          name: `app`,
          dependencies: {
            lib: `workspace:*`,
          },
        });

        await xfs.mkdirpPromise(ppath.join(path, `packages/lib` as PortablePath));
        await xfs.writeJsonPromise(ppath.join(path, `packages/lib/package.json` as PortablePath), {
          name: `lib`,
          dependencies: {
            [`no-deps`]: `1.0.0`,
          },
        });

        await run(`install`);
        await run(`workspaces`, `focus`, `app`, {cwd: path});

        const installStatePath = ppath.join(path, `.yarn/ignore/install` as PortablePath);
        const stateBefore = await xfs.statPromise(installStatePath);

        await new Promise(resolve => setTimeout(resolve, 10));

        await run(`node`, `-e`, `require('no-deps')`, {
          cwd: ppath.join(path, `packages/lib` as PortablePath),
        });

        const stateAfter = await xfs.statPromise(installStatePath);
        expect(stateAfter.mtimeMs).toEqual(stateBefore.mtimeMs);
      }),
    );

    test(
      `it should include optional workspace dependencies in focused install coverage`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await xfs.writeJsonPromise(ppath.join(path, Filename.manifest), {
          private: true,
          workspaces: [`packages/*`],
        });

        await xfs.mkdirpPromise(ppath.join(path, `packages/app` as PortablePath));
        await xfs.writeJsonPromise(ppath.join(path, `packages/app/package.json` as PortablePath), {
          name: `app`,
          optionalDependencies: {
            lib: `workspace:*`,
          },
        });

        await xfs.mkdirpPromise(ppath.join(path, `packages/lib` as PortablePath));
        await xfs.writeJsonPromise(ppath.join(path, `packages/lib/package.json` as PortablePath), {
          name: `lib`,
          dependencies: {
            [`no-deps`]: `1.0.0`,
          },
        });

        await run(`install`);
        await run(`workspaces`, `focus`, `app`, {cwd: path});

        const installStatePath = ppath.join(path, `.yarn/ignore/install` as PortablePath);
        const stateBefore = await xfs.statPromise(installStatePath);

        await new Promise(resolve => setTimeout(resolve, 10));

        await run(`node`, `-e`, `require('no-deps')`, {
          cwd: ppath.join(path, `packages/lib` as PortablePath),
        });

        const stateAfter = await xfs.statPromise(installStatePath);
        expect(stateAfter.mtimeMs).toEqual(stateBefore.mtimeMs);
      }),
    );

    test(
      `it should fall back to a full install when a fresh focused install has an incomplete stale lockfile`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await setupMonorepo(path);
        await run(`install`);

        const cacheFolder = ppath.join(path, `.yarn/cache` as PortablePath);
        await xfs.removePromise(cacheFolder);

        await run(`workspaces`, `focus`, `foo`, {cwd: ppath.join(path, `packages/foo` as PortablePath)});

        const lockfilePath = ppath.join(path, `yarn.lock` as PortablePath);
        const lockfile = await xfs.readJsonPromise(lockfilePath);
        delete lockfile.entries[`one-fixed-dep@npm:1.0.0`];
        await xfs.writeJsonPromise(lockfilePath, lockfile);

        await run(`node`, `-e`, `require('no-deps')`, {
          cwd: ppath.join(path, `packages/bar` as PortablePath),
        });

        await expect(source(`require('one-fixed-dep')`, {
          cwd: ppath.join(path, `packages/baz` as PortablePath),
        })).resolves.toMatchObject({
          name: `one-fixed-dep`,
          version: `1.0.0`,
        });

        await expect(getCacheContent(cacheFolder)).resolves.toEqual([
          expect.stringContaining(`no-deps-npm-1.0.0-`),
          expect.stringContaining(`no-deps-npm-2.0.0-`),
          expect.stringContaining(`one-fixed-dep-npm-1.0.0-`),
        ]);
      }),
    );

    test(
      `it should use a focused reinstall when a focused install is stale but the lockfile is fresh`,
      makeTemporaryEnv({}, async ({path, run}) => {
        await setupMonorepo(path);
        await run(`install`);

        const cacheFolder = ppath.join(path, `.yarn/cache` as PortablePath);
        await xfs.removePromise(cacheFolder);

        await run(`workspaces`, `focus`, `foo`, {cwd: ppath.join(path, `packages/foo` as PortablePath)});

        const fooManifestPath = ppath.join(path, `packages/foo/package.json` as PortablePath);
        await xfs.writeJsonPromise(fooManifestPath, await xfs.readJsonPromise(fooManifestPath));

        await run(`node`, `-e`, `require('no-deps')`, {
          cwd: ppath.join(path, `packages/bar` as PortablePath),
        });

        await expect(getCacheContent(cacheFolder)).resolves.toEqual([
          expect.stringContaining(`no-deps-npm-1.0.0-`),
          expect.stringContaining(`no-deps-npm-2.0.0-`),
        ]);
      }),
    );

    test(
      `it should fall back to a full install when a focused install is stale and the lockfile is stale`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await setupMonorepo(path);
        await run(`install`);
        await run(`workspaces`, `focus`, `foo`, {cwd: ppath.join(path, `packages/foo` as PortablePath)});

        const barManifestPath = ppath.join(path, `packages/bar/package.json` as PortablePath);
        const barManifest = await xfs.readJsonPromise(barManifestPath);
        barManifest.dependencies[`one-fixed-dep`] = `1.0.0`;
        await xfs.writeJsonPromise(barManifestPath, barManifest);

        await run(`node`, `-e`, `require('one-fixed-dep')`, {
          cwd: ppath.join(path, `packages/bar` as PortablePath),
        });

        await expect(source(`require('one-fixed-dep')`, {
          cwd: ppath.join(path, `packages/baz` as PortablePath),
        })).resolves.toMatchObject({
          name: `one-fixed-dep`,
          version: `1.0.0`,
        });
      }),
    );

    test(
      `it should not write install state in update-lockfile mode`,
      makeTemporaryEnv({}, async ({path, run}) => {
        await setupMonorepo(path);
        await run(`install`);
        await run(`workspaces`, `focus`, `foo`, {cwd: ppath.join(path, `packages/foo` as PortablePath)});

        const installStatePath = ppath.join(path, `.yarn/ignore/install` as PortablePath);
        const stateBefore = await xfs.statPromise(installStatePath);

        await new Promise(resolve => setTimeout(resolve, 10));

        await run(`install`, `--mode=update-lockfile`);

        const stateAfter = await xfs.statPromise(installStatePath);
        expect(stateAfter.mtimeMs).toEqual(stateBefore.mtimeMs);
      }),
    );
  });
});
