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
      `it should resolve dependencies from an authenticated Simple API registry`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`pypi-one-dep`]: `pypi:1.0.0`,
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();
          const privateRegistryUrl = `${registryUrl}/private-pypi`;

          await yarn.writeConfiguration(path, {
            packageRules: [{
              ecosystemFilter: `pypi`,
              packageFilter: `pypi-*`,
              pypiRegistryServer: privateRegistryUrl,
            }],
            sourceRules: [{
              ecosystemFilter: `pypi`,
              registryFilter: privateRegistryUrl,
              pypiAuthIdent: tests.validLogins.fooUser.npmAuthIdent.decoded,
            }],
          });

          await run(`install`);

          const cacheEntries = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));
          expect(cacheEntries.some(entry => entry.includes(`pypi-one-dep-pypi-1.0.0`))).toBe(true);
          expect(cacheEntries.some(entry => entry.includes(`pypi-no-deps-pypi-1.1.0`))).toBe(true);
        },
      ),
    );

    test(
      `it should only select a prerelease when the range opts into prereleases`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`pypi-no-deps`]: `pypi:>=2.0.0rc1`,
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();

          await yarn.writeConfiguration(path, {
            pypiRegistryServer: registryUrl,
          });

          await run(`install`);

          const cacheEntries = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));
          expect(cacheEntries.some(entry => entry.includes(`pypi-no-deps-pypi-2.0.0rc1`))).toEqual(true);
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
      `it should normalize PyPI extras when evaluating extra markers`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`pypi-extra-provider`]: `pypi:1.0.0#extras=feature-name`,
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
          expect(cacheEntries.some(entry => entry.includes(`pypi-entry-points-pypi-1.0.0`))).toEqual(true);
          expect(cacheEntries.some(entry => entry.includes(`pypi-no-deps`))).toEqual(false);
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
      `it should reject conflicting PyPI extra requirements for the same dependency`,
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

          const cacheEntries = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));

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

          await expect(run(`install`)).rejects.toThrow();
        },
      ),
    );

    test(
      `it should intersect overlapping PyPI extra requirements for the same dependency`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`pypi-extra-narrows-base`]: `pypi:1.0.0#extras=feature`,
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();

          await yarn.writeConfiguration(path, {
            pypiRegistryServer: registryUrl,
          });

          await run(`install`);

          const cacheEntries = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));

          expect(cacheEntries.some(entry => entry.includes(`pypi-extra-narrows-base-pypi-1.0.0`))).toEqual(true);
          expect(cacheEntries.some(entry => entry.includes(`pypi-no-deps-pypi-1.0.0`))).toEqual(true);
          expect(cacheEntries.some(entry => entry.includes(`pypi-no-deps-pypi-1.1.0`))).toEqual(false);
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
      `it should keep lockfiles stable when the same PyPI version is requested with and without extras`,
      makeTemporaryMonorepoEnv(
        {workspaces: [`packages/*`]},
        {
          [`packages/base`]: {
            name: `base`,
            version: `1.0.0`,
            dependencies: {
              [`pypi-extra-provider`]: `pypi:1.0.0`,
            },
          },
          [`packages/extra`]: {
            name: `extra`,
            version: `1.0.0`,
            dependencies: {
              [`pypi-extra-provider`]: `pypi:1.0.0#extras=feature`,
            },
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();
          await yarn.writeConfiguration(path, {
            pypiRegistryServer: registryUrl,
          });

          await run(`install`);
          await run(`install`, `--immutable`);
        },
      ),
    );

    test(
      `it should resolve PyPI extras through target-qualified venv islands`,
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
            supportedTargets: [{
              os: process.platform,
              cpu: process.arch,
              python: {
                version: `3.12`,
              },
            }],
            unstableIslands: {
              main: {
                workspaces: [`island-ws`],
                linker: `venv`,
              },
            },
          });

          await run(`install`);

          await expect(xfs.existsPromise(ppath.join(path, `packages/island-ws/.venv/lib/site-packages/pypi-no-deps/pypi_no_deps/__init__.py` as any))).resolves.toEqual(true);

          const {stdout} = await run(
            `python`,
            `-c`,
            `import pypi_no_deps; print(pypi_no_deps.VALUE)`,
            {cwd: ppath.join(path, `packages/island-ws`)},
          );

          expect(stdout.trim()).toEqual(`1.1.0`);
        },
      ),
    );

    test(
      `it should reject conflicting PyPI extra requirements through venv islands`,
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

          await expect(run(`install`)).rejects.toThrow();
        },
      ),
    );

    test(
      `it should keep base dependencies when a deeper transitive dependency later requests PyPI extras`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/island-ws`]: {
            name: `island-ws`,
            version: `1.0.0`,
            dependencies: {
              [`pypi-base-with-base-forwarder`]: `pypi:1.0.0`,
              [`pypi-extra-with-base-chain-forwarder`]: `pypi:1.0.0`,
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

          await expect(xfs.existsPromise(ppath.join(path, `packages/island-ws/.venv/lib/site-packages/pypi-entry-points/pypi_entry_points/__init__.py` as any))).resolves.toEqual(true);
          await expect(xfs.existsPromise(ppath.join(path, `packages/island-ws/.venv/lib/site-packages/pypi-no-deps/pypi_no_deps/__init__.py` as any))).resolves.toEqual(true);
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
