import {ppath, xfs} from '@yarnpkg/fslib';
import {tests}      from 'pkg-tests-core';

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

          await run(`install`, {
            env: {
              ZPM_PYPI_REGISTRY: registryUrl,
            },
          });

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

          await run(`install`, {
            env: {
              ZPM_PYPI_REGISTRY: registryUrl,
            },
          });

          const cacheEntries = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));
          const zipEntries = cacheEntries.filter(entry => entry.endsWith(`.zip`));

          expect(zipEntries.some(entry => entry.includes(`pypi-no-deps-pypi-1.1.0`))).toEqual(true);
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

          await run(`install`, {
            env: {
              ZPM_PYPI_REGISTRY: registryUrl,
            },
          });

          const cacheEntries = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));
          const zipEntries = cacheEntries.filter(entry => entry.endsWith(`.zip`));

          expect(zipEntries.some(entry => entry.includes(`pypi-one-dep-pypi-1.0.0`))).toEqual(true);
          expect(zipEntries.some(entry => entry.includes(`pypi-no-deps-pypi-1.1.0`))).toEqual(true);
          expect(zipEntries.some(entry => entry.includes(`marker-only-dep`))).toEqual(false);
        },
      ),
    );
  });
});
