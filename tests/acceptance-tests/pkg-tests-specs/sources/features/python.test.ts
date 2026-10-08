import {Filename, PortablePath, ppath, xfs} from '@yarnpkg/fslib';
import {exec, tests, yarn}                  from 'pkg-tests-core';

const {PYPI_AUTH_TOKEN} = tests;

async function setupVenvProject(path: PortablePath, config: Record<string, any> = {}) {
  const registryUrl = await tests.startPackageServer();

  await yarn.writeConfiguration(path, {
    pypiRegistryServer: `${registryUrl}/simple/`,
    unstableIslands: {
      main: {workspaces: [`app`], linker: `venv`},
    },
    ...config,
  });

  return registryUrl;
}

async function pythonEval(run: any, path: PortablePath, code: string) {
  const {stdout} = await run(`python`, `-c`, code, {cwd: ppath.join(path, `app` as PortablePath)});
  return stdout.trim();
}

describe(`Features`, () => {
  describe(`Python`, () => {
    test(`it should resolve through PEP 691, PEP 503, and without PEP 658 metadata`, makeTemporaryEnv({}, async ({path, run}) => {
      const registryUrl = await tests.startPackageServer();

      for (const flavor of [`simple`, `simple-html`, `simple-nometa`]) {
        await xfs.removePromise(ppath.join(path, `yarn.lock`));
        await xfs.removePromise(ppath.join(path, `.yarn`));

        await xfs.writeJsonPromise(ppath.join(path, Filename.manifest), {
          dependencies: {[`pypi-one-dep`]: `pypi:1.0.0`},
        });

        await yarn.writeConfiguration(path, {
          pypiRegistryServer: `${registryUrl}/${flavor}/`,
        });

        await run(`install`);

        const cacheEntries = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));
        expect(cacheEntries.some(entry => entry.includes(`pypi-no-deps-pypi-1.1.0`))).toEqual(true);
      }
    }));

    test(`it should not fetch packages with platform variants outside islands`, makeTemporaryEnv({
      // A regular workspace reaches a platform-only
      // package (pywin32) through the greedy resolution
      dependencies: {[`pypi-windows-dep`]: `pypi:1.0.0`},
    }, async ({path, run}) => {
      const registryUrl = await tests.startPackageServer();
      await yarn.writeConfiguration(path, {
        pypiRegistryServer: `${registryUrl}/simple/`,
        supportedArchitectures: [{os: [`darwin`], cpu: [`arm64`, `x64`]}, {os: [`linux`], cpu: [`x64`, `arm64`], libc: [`glibc`]}, {os: [`win32`], cpu: [`x64`]}],
      });

      await run(`install`);

      const lockfile = await xfs.readFilePromise(ppath.join(path, `yarn.lock`), `utf8`);
      expect(lockfile).toContain(`pypi-windows-only`);
    }));

    test(`it should read dependencies from PEP 658 metadata without downloading wheels`, makeTemporaryEnv({
      dependencies: {[`pypi-one-dep`]: `pypi:1.0.0`},
    }, async ({path, run}) => {
      const registryUrl = await tests.startPackageServer();
      await yarn.writeConfiguration(path, {pypiRegistryServer: `${registryUrl}/simple/`});

      tests.pypiMetadataRequests.length = 0;
      await run(`install`);

      expect(tests.pypiMetadataRequests).toContain(`pypi_one_dep-1.0.0-py3-none-any.whl`);
    }));

    test(`it should authenticate against protected indexes`, makeTemporaryEnv({
      dependencies: {[`pypi-no-deps`]: `pypi:1.0.0`},
    }, async ({path, run}) => {
      const registryUrl = await tests.startPackageServer();

      await yarn.writeConfiguration(path, {pypiRegistryServer: `${registryUrl}/simple-auth/`});
      await expect(run(`install`)).rejects.toThrow();

      await yarn.writeConfiguration(path, {
        pypiRegistryServer: `${registryUrl}/simple-auth/`,
        pypiAuthToken: `\${TEST_PYPI_TOKEN}`,
      });

      await run(`install`, {env: {TEST_PYPI_TOKEN: PYPI_AUTH_TOKEN}});

      const cacheEntries = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));
      expect(cacheEntries.some(entry => entry.includes(`pypi-no-deps-pypi-1.0.0`))).toEqual(true);
    }));

    test(`it should route packages to their index through packageRules`, makeTemporaryEnv({
      dependencies: {[`pypi-no-deps`]: `pypi:1.0.0`, [`pypi-entry-points`]: `pypi:1.0.0`},
    }, async ({path, run}) => {
      const registryUrl = await tests.startPackageServer();

      await yarn.writeConfiguration(path, {
        pypiRegistryServer: `${registryUrl}/simple/`,
        packageRules: [{
          ecosystemFilter: `pypi`,
          packageFilter: `pypi-no-deps`,
          pypiRegistryServer: `${registryUrl}/simple-auth/`,
          pypiAuthToken: PYPI_AUTH_TOKEN,
        }],
      });

      await run(`install`);

      const cacheEntries = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));
      expect(cacheEntries.some(entry => entry.includes(`pypi-no-deps-pypi-1.0.0`))).toEqual(true);
      expect(cacheEntries.some(entry => entry.includes(`pypi-entry-points-pypi-1.0.0`))).toEqual(true);
    }));

    test(`it should honor pypiMinimalAgeGate`, makeTemporaryMonorepoEnv({
      workspaces: [`app`],
    }, {
      [`app`]: {name: `app`, version: `1.0.0`, dependencies: {[`pypi-age-gated`]: `pypi:*`, [`pypi-yanked`]: `pypi:*`}},
    }, async ({path, run}) => {
      await setupVenvProject(path, {pypiMinimalAgeGate: `5d`});
      await run(`install`);

      await expect(pythonEval(run, path, `import pypi_age_gated, pypi_yanked; print(pypi_age_gated.VALUE, pypi_yanked.VALUE)`)).resolves.toEqual(`1.1.0 1.0.0`);
    }));

    test(`it should backtrack across PyPI versions`, makeTemporaryMonorepoEnv({
      workspaces: [`app`],
    }, {
      [`app`]: {name: `app`, version: `1.0.0`, dependencies: {[`pypi-conflict-a`]: `pypi:*`, [`pypi-no-deps`]: `pypi:<1.1`}},
    }, async ({path, run}) => {
      await setupVenvProject(path);
      await run(`install`);

      await expect(pythonEval(run, path, `import pypi_conflict_a, pypi_no_deps; print(pypi_conflict_a.VALUE, pypi_no_deps.VALUE)`)).resolves.toEqual(`1.0.0 1.0.0`);
    }));

    test(`it should apply the island's overrides to the requirements of local projects`, makeTemporaryMonorepoEnv({
      workspaces: [`app`, `lib`],
    }, {
      // A project overriding the range its path dependency declares
      // (uv's override-dependencies)
      [`app`]: {name: `app`, version: `1.0.0`, dependencies: {[`lib`]: `workspace:^`, [`pypi-no-deps`]: `pypi:==1.1.0`}, resolutions: {[`pypi-no-deps`]: `pypi:==1.1.0`}},
      [`lib`]: {name: `lib`, version: `1.0.0`, dependencies: {[`pypi-no-deps`]: `pypi:<1.1`}},
    }, async ({path, run}) => {
      await setupVenvProject(path);
      await run(`install`);

      await expect(pythonEval(run, path, `import pypi_no_deps; print(pypi_no_deps.VALUE)`)).resolves.toEqual(`1.1.0`);
    }));

    test(`it should use the local project when another dependency requires it by version`, makeTemporaryMonorepoEnv({
      workspaces: [`app`, `lib`, `plugin`],
    }, {
      // The plugin links the local lib (path
      // source), while app asks for it with a version range, which no
      // release on the index satisfies; uv uses the local project for both
      [`app`]: {name: `app`, version: `1.0.0`, dependencies: {[`plugin`]: `workspace:^`, [`pypi-local-lib`]: `pypi:>=9`}},
      [`plugin`]: {name: `plugin`, version: `1.0.0`, dependencies: {[`pypi-local-lib`]: `workspace:^`}},
      [`lib`]: {name: `pypi-local-lib`, version: `1.0.0`},
    }, async ({path, run}) => {
      await xfs.writeFilePromise(ppath.join(path, `lib/pyproject.toml` as PortablePath), `[project]\nname = "pypi-local-lib"\nversion = "1.0.0"\n`);
      await xfs.mkdirpPromise(ppath.join(path, `lib/pypi_local_lib` as PortablePath));
      await xfs.writeFilePromise(ppath.join(path, `lib/pypi_local_lib/__init__.py` as PortablePath), `VALUE = "local"\n`);

      await setupVenvProject(path, {
        unstableIslands: {
          main: {workspaces: [`app`], linker: `venv`},
        },
      });

      await run(`install`);

      await expect(pythonEval(run, path, `import pypi_local_lib; print(pypi_local_lib.VALUE)`)).resolves.toEqual(`local`);
    }));

    test(`it should evaluate markers for the current platform`, makeTemporaryMonorepoEnv({
      workspaces: [`app`],
    }, {
      [`app`]: {name: `app`, version: `1.0.0`, dependencies: {[`pypi-marker-deps`]: `pypi:1.0.0`}},
    }, async ({path, run}) => {
      await setupVenvProject(path, {
        supportedArchitectures: [{os: [`darwin`], cpu: [`arm64`]}, {os: [`linux`], cpu: [`x64`], libc: [`glibc`]}],
      });

      await run(`install`);

      const lockfile = await xfs.readFilePromise(ppath.join(path, `yarn.lock`), `utf8`);
      expect(lockfile).toContain(`pypi-no-deps`);
      expect(lockfile).toContain(`pypi-entry-points`);
      expect(lockfile).not.toContain(`pypi-never`);
      expect(lockfile).not.toContain(`pypi-old-python`);

      const result = JSON.parse(await pythonEval(run, path, `import importlib.util, json; print(json.dumps([importlib.util.find_spec(m) is not None for m in ["pypi_no_deps", "pypi_entry_points"]]))`));
      expect(result).toEqual(process.platform === `darwin` ? [true, false] : [false, true]);
    }));

    test(`it should install a wheel shared by several platforms on each of them`, makeTemporaryMonorepoEnv({
      workspaces: [`app`],
    }, {
      [`app`]: {name: `app`, version: `1.0.0`, dependencies: {[`pypi-universal2`]: `pypi:1.0.0`}},
    }, async ({path, run}) => {
      // Both macOS architectures get the same universal2 wheel; whichever
      // the tests run on must find it
      await setupVenvProject(path, {
        supportedArchitectures: [{os: [`darwin`], cpu: [`arm64`, `x64`]}, {os: [`linux`], cpu: [`x64`, `arm64`], libc: [`glibc`]}, {os: [`win32`], cpu: [`x64`]}],
      });

      await run(`install`);

      await expect(pythonEval(run, path, `import pypi_universal2; print(pypi_universal2.VALUE)`)).resolves.toEqual(`1.0.0`);
    }));

    test(`it should skip packages without artifacts for the current platform when their markers exclude it`, makeTemporaryMonorepoEnv({
      workspaces: [`app`],
    }, {
      [`app`]: {name: `app`, version: `1.0.0`, dependencies: {[`pypi-windows-dep`]: `pypi:1.0.0`}},
    }, async ({path, run}) => {
      await setupVenvProject(path, {
        supportedArchitectures: [{os: [`darwin`], cpu: [`arm64`, `x64`]}, {os: [`linux`], cpu: [`x64`, `arm64`], libc: [`glibc`]}, {os: [`win32`], cpu: [`x64`]}],
      });

      await run(`install`);

      // Locked for Windows, never installed elsewhere
      const lockfile = await xfs.readFilePromise(ppath.join(path, `yarn.lock`), `utf8`);
      expect(lockfile).toContain(`pypi-windows-only`);

      const result = JSON.parse(await pythonEval(run, path, `import importlib.util, json; print(json.dumps([importlib.util.find_spec(m) is not None for m in ["pypi_windows_dep", "pypi_no_deps", "pypi_windows_only"]]))`));
      expect(result).toEqual([true, true, false]);
    }));

    test(`it should lock platform-specific wheels for every supported architecture but only fetch the current one`, makeTemporaryMonorepoEnv({
      workspaces: [`app`],
    }, {
      [`app`]: {name: `app`, version: `1.0.0`, dependencies: {[`pypi-platform-specific`]: `pypi:1.0.0`}},
    }, async ({path, run}) => {
      await setupVenvProject(path, {
        pythonVersion: `3.12`,
        supportedArchitectures: [{os: [`darwin`], cpu: [`arm64`, `x64`]}, {os: [`linux`], cpu: [`x64`, `arm64`], libc: [`glibc`]}],
      });

      // The link step would need a Python 3.12 interpreter, which isn't
      // needed to check what gets locked and fetched
      await run(`install`, `--mode=update-lockfile`, {env: {YARN_PYTHON_VERSION: `3.12`}});

      const lockfile = await xfs.readFilePromise(ppath.join(path, `yarn.lock`), `utf8`);
      for (const tag of [`macosx_11_0_arm64`, `macosx_10_12_x86_64`, `manylinux_2_17_x86_64`, `manylinux_2_17_aarch64`])
        expect(lockfile).toContain(`cp312-cp312-${tag}`);

      expect(lockfile).not.toContain(`cp311`);
      expect(lockfile).not.toContain(`cp313`);

      const allEntries = (await xfs.readdirPromise(ppath.join(path, `.yarn/cache`))).filter(entry => entry.startsWith(`pypi-platform-specific`));
      expect(allEntries).toHaveLength(4);

      // Same lockfile, but only the current architecture is supported: a
      // single artifact gets downloaded, and the lockfile doesn't change
      await xfs.removePromise(ppath.join(path, `.yarn/cache`));
      await yarn.writeConfiguration(path, {
        ...(await yarn.readConfiguration(path)),
        supportedArchitectures: undefined,
      });

      await run(`install`, `--mode=update-lockfile`, {env: {YARN_PYTHON_VERSION: `3.12`}});

      const currentEntries = (await xfs.readdirPromise(ppath.join(path, `.yarn/cache`))).filter(entry => entry.startsWith(`pypi-platform-specific`));
      expect(currentEntries).toHaveLength(1);
    }));

    test(`it should create a standard virtualenv`, makeTemporaryMonorepoEnv({
      workspaces: [`app`],
    }, {
      [`app`]: {name: `app`, version: `1.0.0`, dependencies: {[`pypi-entry-points`]: `pypi:1.0.0`, [`pypi-no-deps`]: `pypi:1.0.0`}},
    }, async ({path, run}) => {
      await setupVenvProject(path);
      await run(`install`);

      const venv = ppath.join(path, `app/.venv` as PortablePath);

      expect(await xfs.existsPromise(ppath.join(venv, `pyvenv.cfg`))).toEqual(true);
      expect(await xfs.existsPromise(ppath.join(venv, `bin/pypi-entry-points` as PortablePath))).toEqual(true);

      // The venv must work without going through Yarn
      const {stdout} = await exec.execFile(ppath.join(venv, `bin/python` as PortablePath), [`-c`, `import sys, pypi_no_deps; print(sys.prefix == sys.base_prefix, pypi_no_deps.VALUE)`], {cwd: path, env: {...process.env, PYTHONPATH: ``}} as any);
      expect(stdout.trim()).toEqual(`False 1.0.0`);

      const script = await exec.execFile(ppath.join(venv, `bin/pypi-entry-points` as PortablePath), [`hello`], {cwd: path} as any);
      expect(script.stdout.trim()).toEqual(`hello`);

      // A second install must not rebuild the venv
      const stat = await xfs.statPromise(ppath.join(venv, `pyvenv.cfg`));
      await run(`install`);
      expect((await xfs.statPromise(ppath.join(venv, `pyvenv.cfg`))).mtimeMs).toEqual(stat.mtimeMs);
    }));

    test(`it should install local workspaces as editable`, makeTemporaryMonorepoEnv({
      workspaces: [`app`, `lib`],
    }, {
      [`app`]: {name: `app`, version: `1.0.0`, dependencies: {[`lib`]: `workspace:^`}},
      [`lib`]: {name: `lib`, version: `1.0.0`, dependencies: {[`pypi-no-deps`]: `pypi:1.0.0`}},
    }, async ({path, run}) => {
      await setupVenvProject(path, {
        unstableIslands: {main: {workspaces: [`app`, `lib`], linker: `venv`}},
      });

      await xfs.mkdirpPromise(ppath.join(path, `lib/src/mylib` as PortablePath));
      await xfs.writeFilePromise(ppath.join(path, `lib/pyproject.toml` as PortablePath), `[project]\nname = "lib"\nversion = "1.0.0"\n\n[project.scripts]\nmylib-cli = "mylib:main"\n`);
      await xfs.writeFilePromise(ppath.join(path, `lib/src/mylib/__init__.py` as PortablePath), `import pypi_no_deps\nVALUE = "v1"\ndef main():\n    print("cli", VALUE)\n`);

      await run(`install`);

      await expect(pythonEval(run, path, `import mylib; print(mylib.VALUE)`)).resolves.toEqual(`v1`);

      // (different size, so the cached bytecode is invalidated even within the same second)
      await xfs.writeFilePromise(ppath.join(path, `lib/src/mylib/__init__.py` as PortablePath), `import pypi_no_deps\nVALUE = "v2"\n\ndef main():\n    print("cli", VALUE)\n`);
      await expect(pythonEval(run, path, `import mylib; print(mylib.VALUE)`)).resolves.toEqual(`v2`);

      const script = await exec.execFile(ppath.join(path, `app/.venv/bin/mylib-cli` as PortablePath), [], {cwd: path} as any);
      expect(script.stdout.trim()).toEqual(`cli v2`);
    }));
    test(`it should apply workspace resolutions as overrides in their island`, makeTemporaryMonorepoEnv({
      workspaces: [`app`],
    }, {
      [`app`]: {
        name: `app`,
        version: `1.0.0`,
        dependencies: {[`pypi-one-dep`]: `pypi:1.0.0`},
        resolutions: {[`pypi-no-deps`]: `pypi:==1.0.0`},
      },
    }, async ({path, run}) => {
      await setupVenvProject(path);
      await run(`install`);

      await expect(pythonEval(run, path, `import pypi_no_deps; print(pypi_no_deps.VALUE)`)).resolves.toEqual(`1.0.0`);
    }));

    test(`it should apply pypiConstraints without adding dependencies`, makeTemporaryMonorepoEnv({
      workspaces: [`app`],
    }, {
      [`app`]: {name: `app`, version: `1.0.0`, dependencies: {[`pypi-one-dep`]: `pypi:1.0.0`}},
    }, async ({path, run}) => {
      await setupVenvProject(path, {
        unstableIslands: {main: {workspaces: [`app`], linker: `venv`, pypiConstraints: [`pypi-no-deps<1.1`, `pypi-entry-points>=1`]}},
      });

      await run(`install`);

      await expect(pythonEval(run, path, `import importlib.util, pypi_no_deps; print(pypi_no_deps.VALUE, importlib.util.find_spec("pypi_entry_points") is None)`)).resolves.toEqual(`1.0.0 True`);
    }));
    test(`it should reuse locked islands without querying the index`, makeTemporaryMonorepoEnv({
      workspaces: [`app`],
    }, {
      [`app`]: {name: `app`, version: `1.0.0`, dependencies: {[`pypi-one-dep`]: `pypi:1.0.0`}},
    }, async ({path, run}) => {
      const registryUrl = await setupVenvProject(path);
      await run(`install`);

      await xfs.removePromise(ppath.join(path, `app/.venv` as PortablePath));

      // The index is unreachable; the install must rely on the lockfile,
      // the cache, and the persisted metadata only
      await yarn.writeConfiguration(path, {
        ...(await yarn.readConfiguration(path)),
        pypiRegistryServer: `${registryUrl}/simple/`,
        enableNetwork: false,
      });

      await run(`install`);
      await expect(pythonEval(run, path, `import pypi_no_deps; print(pypi_no_deps.VALUE)`)).resolves.toEqual(`1.1.0`);
    }));
    test(`it should resolve platform wheels per island Python version`, makeTemporaryMonorepoEnv({
      workspaces: [`a`, `b`],
    }, {
      [`a`]: {name: `a`, version: `1.0.0`, dependencies: {[`pypi-platform-specific`]: `pypi:1.0.0`}},
      [`b`]: {name: `b`, version: `1.0.0`, dependencies: {[`pypi-platform-specific`]: `pypi:1.0.0`}},
    }, async ({path, run}) => {
      const registryUrl = await tests.startPackageServer();

      await yarn.writeConfiguration(path, {
        pypiRegistryServer: `${registryUrl}/simple/`,
        supportedArchitectures: [{os: [`darwin`], cpu: [`arm64`]}, {os: [`linux`], cpu: [`x64`], libc: [`glibc`]}],
        unstableIslands: {
          a: {workspaces: [`a`], linker: `venv`, pythonVersion: `3.12`},
          b: {workspaces: [`b`], linker: `venv`, pythonVersion: `3.13`},
        },
      });

      await run(`install`, `--mode=update-lockfile`);

      const lockfile = await xfs.readFilePromise(ppath.join(path, `yarn.lock`), `utf8`);
      expect(lockfile).toContain(`cp312-cp312-macosx_11_0_arm64`);
      expect(lockfile).toContain(`cp313-cp313-macosx_11_0_arm64`);
      expect(lockfile).toContain(`cp312-cp312-manylinux_2_17_x86_64`);
      expect(lockfile).toContain(`cp313-cp313-manylinux_2_17_x86_64`);

      // The locked islands are reused as-is
      await run(`install`, `--mode=update-lockfile`);
      expect(await xfs.readFilePromise(ppath.join(path, `yarn.lock`), `utf8`)).toEqual(lockfile);
    }));
  });
});
