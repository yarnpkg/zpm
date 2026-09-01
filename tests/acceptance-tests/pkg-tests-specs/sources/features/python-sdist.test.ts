import {Filename, PortablePath, ppath, xfs} from '@yarnpkg/fslib';
import {tests, yarn}                       from 'pkg-tests-core';
import {execFileSync}                      from 'child_process';

function currentPythonVersion() {
  const candidates = [process.env.ZPM_PYTHON_EXECUTABLE, `python3`, `python`]
    .filter((candidate): candidate is string => typeof candidate !== `undefined`);

  for (const candidate of candidates) {
    try {
      return execFileSync(candidate, [`-c`, `import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}')`], {encoding: `utf8`}).trim();
    } catch {
      // Keep looking for the same interpreter names as the sdist preparer.
    }
  }

  throw new Error(`Python is required to run the sdist acceptance tests`);
}

async function configurePythonIsland(path: PortablePath, registryUrl: string, pythonVersion = currentPythonVersion(), extraConfiguration: Record<string, unknown> = {}) {
  await yarn.writeConfiguration(path, {
    pypiRegistryServer: registryUrl,
    supportedTargets: [{
      os: process.platform,
      cpu: process.arch,
      python: {
        version: pythonVersion,
      },
    }],
    unstableIslands: {
      main: {
        workspaces: [`workspace-a`],
        linker: `venv`,
      },
    },
    ...extraConfiguration,
  });
}

async function writeFixtureWheel(path: PortablePath, registryUrl: string, filename: string) {
  const response = await fetch(`${registryUrl}/repositories/pypi/${filename}`);
  if (!response.ok)
    throw new Error(`Failed to load wheel fixture ${filename}: ${response.status}`);

  const wheelPath = ppath.join(path, `packages/workspace-a/wheels/${filename}` as PortablePath);
  await xfs.mkdirpPromise(ppath.dirname(wheelPath));
  await xfs.writeFilePromise(wheelPath, Buffer.from(await response.arrayBuffer()));
}

describe(`Features`, () => {
  describe(`Python sdists`, () => {
    test(
      `it should install a relative local wheel and resolve its metadata dependencies`,
      makeTemporaryMonorepoEnv(
        {workspaces: [`packages/*`]},
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`pypi-one-dep`]: `pypi-file:./wheels/pypi_one_dep-1.0.0-py3-none-any.whl`,
            },
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();
          await configurePythonIsland(path, registryUrl, `3.12`);
          await writeFixtureWheel(path, registryUrl, `pypi_one_dep-1.0.0-py3-none-any.whl`);

          await run(`install`);

          const {stdout} = await run(
            `python`,
            `-c`,
            [
              `import json, pypi_one_dep, pypi_no_deps`,
              `print(json.dumps({"local": pypi_one_dep.VALUE, "dependency": pypi_no_deps.VALUE}))`,
            ].join(`\n`),
            {cwd: `${path}/packages/workspace-a` as PortablePath},
          );

          expect(JSON.parse(stdout.trim())).toEqual({
            local: `one-dep`,
            dependency: `1.1.0`,
          });

          const lockfile = await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`);
          expect(lockfile).toContain(`file:./wheels/pypi_one_dep-1.0.0-py3-none-any.whl#checksum=`);
          expect(lockfile).toContain(`::parent=workspace-a@workspace:workspace-a`);
        },
      ),
    );

    test(
      `it should reject a local wheel whose metadata name differs from the dependency`,
      makeTemporaryMonorepoEnv(
        {workspaces: [`packages/*`]},
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`different-name`]: `pypi-file:./wheels/pypi_one_dep-1.0.0-py3-none-any.whl`,
            },
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();
          await configurePythonIsland(path, registryUrl);
          await writeFixtureWheel(path, registryUrl, `pypi_one_dep-1.0.0-py3-none-any.whl`);

          await expect(run(`install`)).rejects.toThrow(/contains package `pypi-one-dep`, but is required as `different-name`/);
        },
      ),
    );

    test(
      `it should build an sdist with its PEP 517 backend and install the resulting wheel`,
      makeTemporaryMonorepoEnv(
        {workspaces: [`packages/*`]},
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`pypi-sdist`]: `pypi:1.0.0`,
            },
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();
          await configurePythonIsland(path, registryUrl);

          await run(`install`);

          const {stdout} = await run(
            `python`,
            `-c`,
            [
              `import json, pypi_sdist`,
              `print(json.dumps({"value": pypi_sdist.VALUE, "dependency": pypi_sdist.DEPENDENCY_VALUE}))`,
            ].join(`\n`),
            {cwd: `${path}/packages/workspace-a` as PortablePath},
          );

          expect(JSON.parse(stdout.trim())).toEqual({
            value: `built-from-sdist`,
            dependency: `1.0.0`,
          });

          const lockfile = await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`);
          expect(lockfile).toContain(`pypi_sdist-1.0.0.tar.gz`);

          const cacheEntries = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));
          expect(cacheEntries.some(entry => entry.includes(`pypi-sdist`) && entry.endsWith(`-sdist-v1.zip`))).toBe(true);
        },
      ),
    );

    test(
      `it should reuse the prepared wheel without downloading or rebuilding the sdist`,
      makeTemporaryMonorepoEnv(
        {workspaces: [`packages/*`]},
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`pypi-sdist`]: `pypi:1.0.0`,
            },
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();
          await configurePythonIsland(path, registryUrl);

          await run(`install`);
          await xfs.removePromise(ppath.join(path, `packages/workspace-a/.venv` as PortablePath));

          const requests = await tests.startRegistryRecording(async () => {
            await run(`install`, {
              env: {
                ZPM_PYTHON_EXECUTABLE: ppath.join(path, `missing-system-python` as PortablePath),
              },
            });
          });

          expect(requests.filter(request => request.type === tests.RequestType.Repository)).toHaveLength(0);
        },
      ),
    );

    test(
      `it should build sdists with the selected managed Python`,
      makeTemporaryMonorepoEnv(
        {workspaces: [`packages/*`]},
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`@yarnpkg/python`]: `builtin:3.12.4`,
              [`pypi-sdist`]: `pypi:1.0.0`,
            },
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();
          await configurePythonIsland(path, registryUrl, `3.12`);

          await run(`install`, {
            env: {
              ZPM_PYTHON_EXECUTABLE: ppath.join(path, `missing-system-python` as PortablePath),
            },
          });

          const {stdout} = await run(
            `python`,
            `-c`,
            `import pypi_sdist; print(pypi_sdist.VALUE)`,
            {cwd: `${path}/packages/workspace-a` as PortablePath},
          );

          expect(stdout.trim()).toBe(`built-from-sdist`);
        },
      ),
    );

    test(
      `it should skip building sdists for inactive Python target forks`,
      makeTemporaryMonorepoEnv(
        {workspaces: [`packages/*`]},
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`pypi-sdist`]: `pypi:1.0.0`,
            },
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();
          const pythonVersion = currentPythonVersion();
          const inactiveOs = process.platform === `darwin` ? `linux` : `darwin`;
          await configurePythonIsland(path, registryUrl, pythonVersion, {
            supportedTargets: [{
              os: process.platform,
              cpu: process.arch,
              python: {version: pythonVersion},
            }, {
              os: inactiveOs,
              cpu: process.arch,
              python: {version: pythonVersion},
            }],
          });

          await run(`install`);

          const {stdout} = await run(
            `python`,
            `-c`,
            `import pypi_sdist; print(pypi_sdist.VALUE)`,
            {cwd: `${path}/packages/workspace-a` as PortablePath},
          );

          expect(stdout.trim()).toBe(`built-from-sdist`);
        },
      ),
    );

    test(
      `it should use the configured authenticated registry for build requirements`,
      makeTemporaryMonorepoEnv(
        {workspaces: [`packages/*`]},
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`pypi-private-build-sdist`]: `pypi:1.0.0`,
            },
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();
          await configurePythonIsland(path, registryUrl, currentPythonVersion(), {
            sourceRules: [{
              ecosystemFilter: `pypi`,
              registryFilter: registryUrl,
              pypiAuthIdent: tests.validLogins.fooUser.npmAuthIdent.decoded,
            }],
          });

          await run(`install`);

          const {stdout} = await run(
            `python`,
            `-c`,
            `import pypi_private_build_sdist; print(pypi_private_build_sdist.VALUE)`,
            {cwd: `${path}/packages/workspace-a` as PortablePath},
          );

          expect(stdout.trim()).toBe(`private-build-requirement`);
        },
      ),
    );

    test(
      `it should replace transitive Python constraints with root resolutions`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
          resolutions: {
            [`pypi-no-deps`]: `pypi:==1.1.0`,
          },
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`pypi-extra-overrides-base`]: `pypi:1.0.0#extras=feature`,
            },
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();
          await configurePythonIsland(path, registryUrl);

          await run(`install`);

          const {stdout} = await run(
            `python`,
            `-c`,
            `import pypi_no_deps; print(pypi_no_deps.VALUE)`,
            {cwd: `${path}/packages/workspace-a` as PortablePath},
          );

          expect(stdout.trim()).toBe(`1.1.0`);
        },
      ),
    );

    test(
      `it should build and install a Python project from Git`,
      makeTemporaryMonorepoEnv(
        {workspaces: [`packages/*`]},
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`django-pieuvre`]: `pypi-git:https://gitlab.com/fasfox/django-pieuvre.git#commit=4b377f1190b369de31ec0839e10b89daef06bd2e`,
            },
          },
        },
        async ({path, run}) => {
          const repositoryUrl = `https://gitlab.com/fasfox/django-pieuvre.git`;
          await configurePythonIsland(path, `https://pypi.org`, currentPythonVersion(), {
            approvedGitRepositories: [repositoryUrl],
          });

          await run(`install`);

          const {stdout} = await run(
            `python`,
            `-c`,
            `from importlib.metadata import version; print(version("django-pieuvre"))`,
            {cwd: `${path}/packages/workspace-a` as PortablePath},
          );

          expect(stdout.trim()).toBe(`0.7.2`);
          const lockfile = await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`);
          expect(lockfile).toContain(`pypi-git:${repositoryUrl}#commit=4b377f1190b369de31ec0839e10b89daef06bd2e`);
        },
      ),
    );

    test(
      `it should report PEP 517 backend failures`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`pypi-broken-sdist`]: `pypi:1.0.0`,
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();
          await yarn.writeConfiguration(path, {
            pypiRegistryServer: registryUrl,
          });

          await expect(run(`install`)).rejects.toThrow(/intentional sdist build failure/);
        },
      ),
    );
  });
});
