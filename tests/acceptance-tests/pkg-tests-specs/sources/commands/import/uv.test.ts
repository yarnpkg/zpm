import {PortablePath, ppath, xfs} from '@yarnpkg/fslib';
import {tests, yarn}              from 'pkg-tests-core';

async function writeProject(path: PortablePath, folder: string, pyproject: string, extra: Record<string, string> = {}) {
  const dir = ppath.join(path, folder as PortablePath);
  await xfs.mkdirpPromise(dir);
  await xfs.writeFilePromise(ppath.join(dir, `pyproject.toml` as PortablePath), pyproject);

  for (const [name, content] of Object.entries(extra)) {
    await xfs.mkdirpPromise(ppath.dirname(ppath.join(dir, name as PortablePath)));
    await xfs.writeFilePromise(ppath.join(dir, name as PortablePath), content);
  }
}

describe(`Commands`, () => {
  describe(`import uv`, () => {
    test(`it should install the project's own extras with --all-extras, at the versions uv locked`, makeTemporaryEnv({
      name: `root`,
      private: true,
    }, async ({path, run}) => {
      const registryUrl = await tests.startPackageServer();

      await yarn.writeConfiguration(path, {
        pypiRegistryServer: `${registryUrl}/simple/`,
      });

      // uv locked pypi-no-deps 1.0.0, although 1.1.0 is available; it's only
      // reached through an extra of the project
      await writeProject(path, `app`, [
        `[project]`,
        `name = "app"`,
        `dependencies = ["pypi-entry-points"]`,
        ``,
        `[project.optional-dependencies]`,
        `feature = ["pypi-no-deps"]`,
      ].join(`\n`), {
        [`uv.lock`]: `version = 1\n\n[[package]]\nname = "pypi-no-deps"\nversion = "1.0.0"\nsource = { registry = "x" }\n`,
      });

      // A first import and install without the extras writes a lockfile...
      await run(`import`, `uv`, `app`);
      await run(`install`);

      // ...then the extras are requested: the seed still applies to the
      // packages the lockfile doesn't know yet
      await xfs.removePromise(ppath.join(path, `app/package.json` as PortablePath));
      await run(`import`, `uv`, `app`, `--all-extras`);

      const app = await xfs.readJsonPromise(ppath.join(path, `app/package.json` as PortablePath));
      expect(app.devDependencies).toHaveProperty([`pypi-no-deps`]);

      await run(`install`);

      const {stdout} = await run(`python`, `-c`, `import pypi_no_deps; print(pypi_no_deps.VALUE)`, {cwd: ppath.join(path, `app` as PortablePath)});
      expect(stdout.trim()).toEqual(`1.0.0`);
    }));

    test(`it should give uv workspace members the sources of their workspace root`, makeTemporaryEnv({
      name: `root`,
      private: true,
    }, async ({path, run}) => {
      const registryUrl = await tests.startPackageServer();

      await yarn.writeConfiguration(path, {
        pypiRegistryServer: `${registryUrl}/simple/`,
      });

      // Like crucible: the root maps a package to a local path; the member
      // only lists the requirement
      await writeProject(path, `suite`, [
        `[project]`,
        `name = "suite"`,
        ``,
        `[tool.uv.workspace]`,
        `members = ["apps/*"]`,
        ``,
        `[tool.uv.sources]`,
        `shared-lib = { path = "../libs/shared", editable = true }`,
      ].join(`\n`));

      await writeProject(path, `suite/apps/worker`, [
        `[project]`,
        `name = "worker"`,
        `dependencies = ["shared-lib"]`,
      ].join(`\n`));

      await writeProject(path, `libs/shared`, [
        `[project]`,
        `name = "shared-lib"`,
      ].join(`\n`));

      await run(`import`, `uv`, `suite`, `--recursive`);

      const worker = await xfs.readJsonPromise(ppath.join(path, `suite/apps/worker/package.json` as PortablePath));
      expect(worker.dependencies).toMatchObject({[`shared-lib`]: `workspace:^`});
    }));

    test(`it should import uv projects and their local dependencies`, makeTemporaryEnv({
      name: `root`,
      private: true,
    }, async ({path, run}) => {
      const registryUrl = await tests.startPackageServer();

      await yarn.writeConfiguration(path, {
        pypiRegistryServer: `${registryUrl}/simple/`,
      });

      await writeProject(path, `services/app`, [
        `[project]`,
        `name = "App_Service"`,
        `requires-python = ">=3.10"`,
        `dependencies = ["pypi-one-dep>=1.0", "shared-lib", "pypi-marker-deps; sys_platform == 'win32'"]`,
        ``,
        `[dependency-groups]`,
        `dev = ["pypi-entry-points", "app-service[extra]"]`,
        `docs = ["pypi-yanked"]`,
        ``,
        `[project.optional-dependencies]`,
        `extra = ["pypi-age-gated"]`,
        ``,
        `[tool.uv]`,
        `override-dependencies = ["pypi-no-deps==1.0.0"]`,
        `constraint-dependencies = ["pypi-conflict-a<2"]`,
        ``,
        `[tool.uv.sources]`,
        `shared-lib = { path = "../../libs/shared", editable = true }`,
      ].join(`\n`), {
        [`uv.lock`]: `version = 1\n\n[[package]]\nname = "pypi-no-deps"\nversion = "1.0.0"\nsource = { registry = "x" }\n`,
      });

      await writeProject(path, `libs/shared`, [
        `[project]`,
        `name = "shared-lib"`,
        `dependencies = ["pypi-no-deps"]`,
      ].join(`\n`), {
        [`shared_lib/__init__.py`]: `VALUE = "shared"\n`,
      });

      await run(`import`, `uv`, `services/app`, `--recursive`);

      const app = await xfs.readJsonPromise(ppath.join(path, `services/app/package.json` as PortablePath));
      expect(app).toEqual({
        name: `app-service`,
        private: true,
        dependencies: {
          [`pypi-one-dep`]: `pypi:>=1.0`,
          [`shared-lib`]: `workspace:^`,
          [`pypi-marker-deps`]: `pypi:*#marker=sys_platform%20%3D%3D%20%27win32%27`,
        },
        devDependencies: {
          [`pypi-entry-points`]: `pypi:*`,
          [`pypi-age-gated`]: `pypi:*`,
        },
        resolutions: {
          [`pypi-no-deps`]: `pypi:==1.0.0`,
        },
      });

      const root = await xfs.readJsonPromise(ppath.join(path, `package.json` as PortablePath));
      expect(root.workspaces.sort()).toEqual([`libs/shared`, `services/app`]);

      const config = await yarn.readConfiguration(path);
      expect(config.unstableIslands[`app-service`]).toMatchObject({
        workspaces: [`app-service`],
        linker: `venv`,
        pypiConstraints: [`pypi-conflict-a<2`],
        pypiSeedLockfile: `services/app/uv.lock`,
      });

      // requires-python >=3.10 allows 3.12, the preferred version
      if (config.unstableIslands[`app-service`].pythonVersion !== undefined)
        expect(config.unstableIslands[`app-service`].pythonVersion).toEqual(`3.12`);

      // The test environment provides another local Python version
      delete config.unstableIslands[`app-service`].pythonVersion;
      await yarn.writeConfiguration(path, config);

      await run(`install`);

      const {stdout} = await run(`python`, `-c`, `import shared_lib, pypi_no_deps, pypi_entry_points; print(shared_lib.VALUE, pypi_no_deps.VALUE)`, {cwd: ppath.join(path, `services/app` as PortablePath)});
      expect(stdout.trim()).toEqual(`shared 1.0.0`);
    }));

    test(`it should support --dry-run`, makeTemporaryEnv({
      name: `root`,
      private: true,
    }, async ({path, run}) => {
      await writeProject(path, `app`, `[project]\nname = "app"\ndependencies = ["pypi-no-deps"]\n`);

      const {stdout} = await run(`import`, `uv`, `app`, `--dry-run`);
      expect(stdout).toContain(`"pypi-no-deps": "pypi:*"`);
      expect(await xfs.existsPromise(ppath.join(path, `app/package.json` as PortablePath))).toEqual(false);
    }));
  });
});
