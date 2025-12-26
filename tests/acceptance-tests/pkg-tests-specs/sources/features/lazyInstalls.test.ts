import {Filename, PortablePath, ppath, xfs} from '@yarnpkg/fslib';
import {yarn}                               from 'pkg-tests-core';

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
  });
});
