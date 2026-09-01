import {Filename, npath, ppath, PortablePath, xfs} from '@yarnpkg/fslib';
import {tests, yarn}                               from 'pkg-tests-core';

function currentSupportedTarget(version: string) {
  return {
    os: process.platform,
    cpu: process.arch,
    python: {
      version,
    },
  };
}

describe(`Features`, () => {
  describe(`Python Versioning`, () => {
    test(
      `it should make the managed Python available through yarn python`,
      makeTemporaryEnv({
        dependencies: {
          [`@yarnpkg/python`]: `builtin:3.12.4`,
        },
      }, async ({run}) => {
        await run(`install`, {
          env: {
            YARN_CPU_OVERRIDE: `x64`,
            YARN_OS_OVERRIDE: `linux`,
            YARN_LIBC_OVERRIDE: `glibc`,
          },
        });

        const {stdout} = await run(`python`, `--version`);
        expect(stdout.trim()).toBe(`Python 3.12.4`);
      }),
    );

    test(
      `it should by default only fetch the @yarnpkg/python package for the current platform`,
      makeTemporaryEnv({
        dependencies: {
          [`@yarnpkg/python`]: `builtin:3.12.4`,
        },
      }, async ({path, run}) => {
        await run(`install`, {
          env: {
            YARN_CPU_OVERRIDE: `x64`,
            YARN_OS_OVERRIDE: `linux`,
            YARN_LIBC_OVERRIDE: `glibc`,
          },
        });

        const allCachedFiles = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));
        const pythonFiles = allCachedFiles.sort().filter(file => file.startsWith(`@yarnpkg-python-`));

        expect(pythonFiles).toEqual([
          expect.stringMatching(/@yarnpkg-python-linux-x64-glibc-builtin-3\.12\.4-/),
        ]);
      }),
    );

    test(
      `it should create a proper venv around a managed Python`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`@yarnpkg/python`]: `builtin:3.12.4`,
              [`pypi-no-deps`]: `pypi:1.0.0`,
            },
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();

          await yarn.writeConfiguration(path, {
            supportedTargets: [
              currentSupportedTarget(`3.12`),
            ],
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`],
                linker: `venv`,
                python: {
                  linkVersion: `3.12`,
                },
              },
            },
          });

          await run(`install`, {
            env: {
              ZPM_PYPI_REGISTRY: registryUrl,
            },
          });

          const venvPath = npath.toPortablePath(`${path}/packages/workspace-a/.venv`);
          const pyvenvCfg = await xfs.readFilePromise(ppath.join(venvPath, `pyvenv.cfg` as Filename), `utf8`);

          expect(pyvenvCfg).toContain(`include-system-site-packages = false`);
          expect(pyvenvCfg).toContain(`version = 3.12`);

          await expect(xfs.existsPromise(ppath.join(venvPath, `bin/python` as Filename))).resolves.toBe(true);
          await expect(xfs.existsPromise(ppath.join(venvPath, `bin/python3` as Filename))).resolves.toBe(true);
          await expect(xfs.existsPromise(ppath.join(venvPath, `bin/python3.12` as Filename))).resolves.toBe(true);

          const linkedDistInfo = npath.toPortablePath(`${venvPath}/lib/python3.12/site-packages/pypi-no-deps/pypi_no_deps-1.0.0.dist-info/METADATA`);
          const legacyLinkedDistInfo = npath.toPortablePath(`${venvPath}/lib/site-packages/pypi-no-deps/pypi_no_deps-1.0.0.dist-info/METADATA`);

          await expect(xfs.existsPromise(linkedDistInfo)).resolves.toBe(true);
          await expect(xfs.existsPromise(legacyLinkedDistInfo)).resolves.toBe(true);

          const {stdout} = await run(`python`, `--version`, {cwd: `${path}/packages/workspace-a` as PortablePath});
          expect(stdout.trim()).toBe(`Python 3.12.4`);
        },
      ),
    );
  });
});
