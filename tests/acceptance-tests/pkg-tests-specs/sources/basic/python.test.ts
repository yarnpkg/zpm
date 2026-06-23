import {ppath, xfs}  from '@yarnpkg/fslib';
import {tests, yarn} from 'pkg-tests-core';

describe(`Protocols`, () => {
  describe(`pypi:`, () => {
    test(
      `it should install a PyPI package with an explicit version and preserve archive bytes`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`pypi-no-deps`]: `pypi:1.0.0`,
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();

          await yarn.writeConfiguration(path, {
            pypiRegistryServer: registryUrl,
          });

          await run(`install`);

          const cacheDir = ppath.join(path, `.yarn/cache`);
          const cacheEntries = await xfs.readdirPromise(cacheDir);
          const pypiArchive = cacheEntries.find(entry => entry.includes(`pypi-no-deps-pypi-1.0.0`) && entry.endsWith(`.zip`));

          expect(pypiArchive).toBeDefined();

          const cacheBytes = await xfs.readFilePromise(ppath.join(cacheDir, pypiArchive!));
          const wheelResponse = await fetch(`${registryUrl}/repositories/pypi/pypi_no_deps-1.0.0-py3-none-any.whl`);
          const wheelBytes = Buffer.from(await wheelResponse.arrayBuffer());

          expect(cacheBytes).toEqual(wheelBytes);
        },
      ),
    );

    test(
      `it should resolve PyPI ranges against metadata`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`pypi-no-deps`]: `pypi:>=1.0.0`,
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();

          await yarn.writeConfiguration(path, {
            pypiRegistryServer: registryUrl,
          });

          await run(`install`);

          const cacheEntries = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));
          const zipEntries = cacheEntries.filter(entry => entry.endsWith(`.zip`));

          expect(zipEntries.some(entry => entry.includes(`pypi-no-deps-pypi-1.1.0`))).toEqual(true);
        },
      ),
    );

    test(
      `it should resolve PyPI packages through package rules`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`pypi-no-deps`]: `pypi:1.0.0`,
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();

          await yarn.writeConfiguration(path, {
            packageRules: [{
              ecosystemFilter: `pypi`,
              pypiRegistryServer: registryUrl,
            }],
          });

          await run(`install`);

          const cacheEntries = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));
          const zipEntries = cacheEntries.filter(entry => entry.endsWith(`.zip`));

          expect(zipEntries.some(entry => entry.includes(`pypi-no-deps-pypi-1.0.0`))).toEqual(true);
        },
      ),
    );

    test(
      `it should resolve dependencies from requires_dist and ignore marker-bearing requirements`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`pypi-one-dep`]: `pypi:1.0.0`,
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();

          await yarn.writeConfiguration(path, {
            pypiRegistryServer: registryUrl,
          });

          await run(`install`);

          const cacheEntries = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));
          const zipEntries = cacheEntries.filter(entry => entry.endsWith(`.zip`));

          expect(zipEntries.some(entry => entry.includes(`pypi-one-dep-pypi-1.0.0`))).toEqual(true);
          expect(zipEntries.some(entry => entry.includes(`pypi-no-deps-pypi-1.1.0`))).toEqual(true);
          expect(zipEntries.some(entry => entry.includes(`marker-only-dep`))).toEqual(false);
        },
      ),
    );

    test(
      `it should only include extra requirements when requested`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`pypi-extra-provider`]: `pypi:1.0.0`,
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();

          await yarn.writeConfiguration(path, {
            pypiRegistryServer: registryUrl,
          });

          await run(`install`);

          let cacheEntries = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));

          expect(cacheEntries.some(entry => entry.includes(`pypi-extra-provider-pypi-1.0.0`))).toEqual(true);
          expect(cacheEntries.some(entry => entry.includes(`pypi-no-deps`))).toEqual(false);

          await xfs.removePromise(ppath.join(path, `.yarn/cache`));
          await xfs.removePromise(ppath.join(path, `yarn.lock` as any));

          await xfs.writeJsonPromise(ppath.join(path, `package.json` as any), {
            dependencies: {
              [`pypi-extra-provider`]: `pypi:1.0.0#extras=feature`,
            },
          });

          await run(`install`);

          cacheEntries = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));

          expect(cacheEntries.some(entry => entry.includes(`pypi-extra-provider-pypi-1.0.0`))).toEqual(true);
          expect(cacheEntries.some(entry => entry.includes(`pypi-no-deps-pypi-1.1.0`))).toEqual(true);
        },
      ),
    );

    test(
      `it should resolve extras requested by PyPI dependencies`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`pypi-extra-forwarder`]: `pypi:1.0.0`,
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();

          await yarn.writeConfiguration(path, {
            pypiRegistryServer: registryUrl,
          });

          await run(`install`);

          const cacheEntries = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));

          expect(cacheEntries.some(entry => entry.includes(`pypi-extra-forwarder-pypi-1.0.0`))).toEqual(true);
          expect(cacheEntries.some(entry => entry.includes(`pypi-extra-provider-pypi-1.0.0`))).toEqual(true);
          expect(cacheEntries.some(entry => entry.includes(`pypi-no-deps-pypi-1.1.0`))).toEqual(true);
        },
      ),
    );

    test(
      `it should let extra requirements override base requirements for the same dependency`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`pypi-extra-overrides-base`]: `pypi:1.0.0`,
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();

          await yarn.writeConfiguration(path, {
            pypiRegistryServer: registryUrl,
          });

          await run(`install`);

          let cacheEntries = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));

          expect(cacheEntries.some(entry => entry.includes(`pypi-extra-overrides-base-pypi-1.0.0`))).toEqual(true);
          expect(cacheEntries.some(entry => entry.includes(`pypi-no-deps-pypi-1.0.0`))).toEqual(true);
          expect(cacheEntries.some(entry => entry.includes(`pypi-no-deps-pypi-1.1.0`))).toEqual(false);

          await xfs.removePromise(ppath.join(path, `.yarn/cache`));
          await xfs.removePromise(ppath.join(path, `yarn.lock` as any));

          await xfs.writeJsonPromise(ppath.join(path, `package.json` as any), {
            dependencies: {
              [`pypi-extra-overrides-base`]: `pypi:1.0.0#extras=feature`,
            },
          });

          await run(`install`);

          cacheEntries = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));

          expect(cacheEntries.some(entry => entry.includes(`pypi-extra-overrides-base-pypi-1.0.0`))).toEqual(true);
          expect(cacheEntries.some(entry => entry.includes(`pypi-no-deps-pypi-1.0.0`))).toEqual(false);
          expect(cacheEntries.some(entry => entry.includes(`pypi-no-deps-pypi-1.1.0`))).toEqual(true);
        },
      ),
    );

    test(
      `it should union multiple requested PyPI extras from range parameters`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`pypi-extra-provider`]: `pypi:1.0.0#extras=feature%2Ctools`,
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();

          await yarn.writeConfiguration(path, {
            pypiRegistryServer: registryUrl,
          });

          await run(`install`);

          const cacheEntries = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));

          expect(cacheEntries.some(entry => entry.includes(`pypi-extra-provider-pypi-1.0.0`))).toEqual(true);
          expect(cacheEntries.some(entry => entry.includes(`pypi-no-deps-pypi-1.1.0`))).toEqual(true);
          expect(cacheEntries.some(entry => entry.includes(`pypi-entry-points-pypi-1.0.0`))).toEqual(true);
        },
      ),
    );

    test(
      `it should resolve PyPI extras through venv islands`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/island-ws`]: {
            name: `island-ws`,
            version: `1.0.0`,
            dependencies: {
              [`pypi-extra-forwarder`]: `pypi:1.0.0`,
            },
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();

          await yarn.writeConfiguration(path, {
            pypiRegistryServer: registryUrl,
            unstableIslands: {
              main: {
                workspaces: [`island-ws`],
                linker: `venv`,
              },
            },
          });

          await run(`install`);

          await expect(xfs.existsPromise(ppath.join(path, `packages/island-ws/.venv/lib/site-packages/pypi-no-deps/pypi_no_deps/__init__.py` as any))).resolves.toEqual(true);
        },
      ),
    );

    test(
      `it should let extra requirements override base requirements through venv islands`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/island-ws`]: {
            name: `island-ws`,
            version: `1.0.0`,
            dependencies: {
              [`pypi-extra-overrides-base`]: `pypi:1.0.0#extras=feature`,
            },
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();

          await yarn.writeConfiguration(path, {
            pypiRegistryServer: registryUrl,
            unstableIslands: {
              main: {
                workspaces: [`island-ws`],
                linker: `venv`,
              },
            },
          });

          await run(`install`);

          await expect(xfs.readFilePromise(ppath.join(path, `packages/island-ws/.venv/lib/site-packages/pypi-no-deps/pypi_no_deps/__init__.py` as any), `utf8`)).resolves.toContain(`VALUE = '1.1.0'`);
        },
      ),
    );

    test(
      `it should support running install twice with a comma-separated PyPI range`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`pypi-no-deps`]: `pypi:>=1.0.0,<2.0.0`,
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();

          await yarn.writeConfiguration(path, {
            pypiRegistryServer: registryUrl,
          });

          for (let i = 0; i < 2; i++) {
            await run(`install`);
          }
        },
      ),
    );
  });
});
