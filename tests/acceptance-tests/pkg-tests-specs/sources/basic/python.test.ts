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
