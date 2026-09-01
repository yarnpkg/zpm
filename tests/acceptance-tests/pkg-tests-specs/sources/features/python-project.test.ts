import {PortablePath, ppath, xfs} from '@yarnpkg/fslib';
import {tests, yarn}               from 'pkg-tests-core';
import {execFileSync}              from 'child_process';

function currentPythonVersion() {
  for (const candidate of [process.env.ZPM_PYTHON_EXECUTABLE, `python3`, `python`]) {
    if (typeof candidate === `undefined`)
      continue;

    try {
      return execFileSync(candidate, [`-c`, `import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}')`], {encoding: `utf8`}).trim();
    } catch {
      // Keep looking for the same interpreter names as the project preparer.
    }
  }

  throw new Error(`Python is required to run the Python project acceptance tests`);
}

async function configurePythonIsland(path: PortablePath, registryUrl: string) {
  await yarn.writeConfiguration(path, {
    pypiRegistryServer: registryUrl,
    supportedTargets: [{
      os: process.platform,
      cpu: process.arch,
      python: {
        version: currentPythonVersion(),
      },
    }],
    unstableIslands: {
      main: {
        workspaces: [`workspace-a`],
        linker: `venv`,
      },
    },
  });
}

async function writePythonProject(path: PortablePath, backend: string) {
  const workspace = ppath.join(path, `packages/workspace-a` as PortablePath);

  await xfs.writeFilePromise(ppath.join(workspace, `pyproject.toml` as PortablePath), [
    `[project]`,
    `name = "local-project"`,
    `version = "1.2.3"`,
    `dependencies = ["pypi-no-deps>=1"]`,
    ``,
    `[build-system]`,
    `requires = []`,
    `build-backend = "backend"`,
    `backend-path = ["."]`,
    ``,
  ].join(`\n`));
  await xfs.writeFilePromise(ppath.join(workspace, `backend.py` as PortablePath), backend);
  await xfs.mkdirpPromise(ppath.join(workspace, `src` as PortablePath));
  await xfs.writeFilePromise(
    ppath.join(workspace, `src/value.py` as PortablePath),
    [
      `VALUE = "installed-from-workspace"`,
      ``,
      `def main():`,
      `    print("workspace-entry-point")`,
      ``,
    ].join(`\n`),
  );
}

async function writeLocalDependencyProject(path: PortablePath) {
  const workspace = ppath.join(path, `packages/local-dependency` as PortablePath);

  await xfs.writeFilePromise(ppath.join(workspace, `pyproject.toml` as PortablePath), [
    `[project]`,
    `name = "local-dependency"`,
    `version = "4.5.6"`,
    `dependencies = ["pypi-no-deps>=1"]`,
    ``,
    `[build-system]`,
    `requires = []`,
    `build-backend = "backend"`,
    `backend-path = ["."]`,
    ``,
  ].join(`\n`));
  await xfs.writeFilePromise(
    ppath.join(workspace, `backend.py` as PortablePath),
    editableBackend
      .replaceAll(`local_project`, `local_dependency`)
      .replaceAll(`local-project`, `local-dependency`)
      .replaceAll(`1.2.3`, `4.5.6`),
  );
  await xfs.mkdirpPromise(ppath.join(workspace, `src` as PortablePath));
  await xfs.writeFilePromise(
    ppath.join(workspace, `src/value.py` as PortablePath),
    `VALUE = "installed-from-local-dependency"\n`,
  );
}

const editableBackend = String.raw`
import pathlib
import subprocess
import zipfile


def get_requires_for_build_editable(config_settings=None):
    return ["pip"]


def build_editable(wheel_directory, config_settings=None, metadata_directory=None):
    import pip

    subprocess.run(["pip", "--version"], check=True)
    wheel_name = "local_project-1.2.3-py3-none-any.whl"
    wheel_path = pathlib.Path(wheel_directory) / wheel_name
    source = (pathlib.Path(__file__).parent / "src" / "value.py").read_text()
    with zipfile.ZipFile(wheel_path, "w") as wheel:
        wheel.writestr("local_project/__init__.py", source + f'BUILD_FRONTEND = "{pip.__name__}"\n')
        wheel.writestr(
            "local_project-1.2.3.dist-info/METADATA",
            "Metadata-Version: 2.1\nName: local-project\nVersion: 1.2.3\n",
        )
        wheel.writestr(
            "local_project-1.2.3.dist-info/WHEEL",
            "Wheel-Version: 1.0\nGenerator: fixture\nRoot-Is-Purelib: true\nTag: py3-none-any\n",
        )
        wheel.writestr(
            "local_project-1.2.3.dist-info/entry_points.txt",
            "[console_scripts]\nlocal-project-cli = local_project:main\n",
        )
    return wheel_name
`;

describe(`Features`, () => {
  describe(`Python projects`, () => {
    test(
      `it should prepare and install a venv island workspace through PEP 660`,
      makeTemporaryMonorepoEnv(
        {workspaces: [`packages/*`]},
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`pypi-no-deps`]: `pypi:>=1.0.0,<2.0.0`,
            },
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();
          await configurePythonIsland(path, registryUrl);
          await writePythonProject(path, editableBackend);

          await run(`install`);

          const {stdout} = await run(
            `python`,
            `-c`,
            [
              `import importlib.metadata, json, local_project, pypi_no_deps`,
              `print(json.dumps({`,
              `  "local": local_project.VALUE,`,
              `  "build_frontend": local_project.BUILD_FRONTEND,`,
              `  "version": importlib.metadata.version("local-project"),`,
              `  "dependency": pypi_no_deps.VALUE,`,
              `}))`,
            ].join(`\n`),
            {cwd: workspacePath(path)},
          );

          expect(JSON.parse(stdout.trim())).toEqual({
            local: `installed-from-workspace`,
            build_frontend: `pip`,
            version: `1.2.3`,
            dependency: `1.1.0`,
          });

          await expect(xfs.existsPromise(ppath.join(
            workspacePath(path), entryPointPath() as PortablePath,
          ))).resolves.toBe(true);

          await expect(run(
            `exec`,
            `local-project-cli`,
            {cwd: workspacePath(path)},
          )).resolves.toMatchObject({
            stdout: `workspace-entry-point\n`,
          });
        },
      ),
    );

    test(
      `it should remove obsolete local Python console scripts when relinking`,
      makeTemporaryMonorepoEnv(
        {workspaces: [`packages/*`]},
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();
          await configurePythonIsland(path, registryUrl);
          await writePythonProject(path, editableBackend);

          await run(`install`);

          const scriptPath = ppath.join(workspacePath(path), entryPointPath() as PortablePath);
          await expect(xfs.existsPromise(scriptPath)).resolves.toBe(true);

          const backendWithoutEntryPoint = editableBackend.replace(
            String.raw`        wheel.writestr(
            "local_project-1.2.3.dist-info/entry_points.txt",
            "[console_scripts]\nlocal-project-cli = local_project:main\n",
        )
`,
            ``,
          );
          await xfs.writeFilePromise(
            ppath.join(workspacePath(path), `backend.py` as PortablePath),
            backendWithoutEntryPoint,
          );

          await run(`install`);

          await expect(xfs.existsPromise(scriptPath)).resolves.toBe(false);
        },
      ),
    );

    test(
      `it should report local project backend failures`,
      makeTemporaryMonorepoEnv(
        {workspaces: [`packages/*`]},
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();
          await configurePythonIsland(path, registryUrl);
          await writePythonProject(path, String.raw`
def build_editable(wheel_directory, config_settings=None, metadata_directory=None):
    raise RuntimeError("intentional local project build failure")
`);

          await expect(run(`install`)).rejects.toThrow(/intentional local project build failure/);
        },
      ),
    );

    test(
      `it should use the legacy setuptools backend when pyproject.toml has no build-system`,
      makeTemporaryMonorepoEnv(
        {workspaces: [`packages/*`]},
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();
          await configurePythonIsland(path, registryUrl);

          const workspace = workspacePath(path);
          await xfs.writeFilePromise(ppath.join(workspace, `pyproject.toml` as PortablePath), [
            `[project]`,
            `name = "legacy-default"`,
            `version = "1.0.0"`,
            ``,
          ].join(`\n`));
          await xfs.mkdirpPromise(ppath.join(workspace, `legacy_default` as PortablePath));
          await xfs.writeFilePromise(
            ppath.join(workspace, `legacy_default/__init__.py` as PortablePath),
            `VALUE = "setuptools-default"\n`,
          );

          await run(`install`);

          const {stdout} = await run(
            `python`,
            `-c`,
            `import legacy_default; print(legacy_default.VALUE)`,
            {cwd: workspace},
          );
          expect(stdout.trim()).toEqual(`setuptools-default`);
        },
      ),
    );

    test(
      `it should prepare local workspace dependencies into the consuming venv`,
      makeTemporaryMonorepoEnv(
        {workspaces: [`packages/*`]},
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`local-dependency`]: `workspace:*`,
            },
          },
          [`packages/local-dependency`]: {
            name: `local-dependency`,
            version: `4.5.6`,
            dependencies: {
              [`pypi-no-deps`]: `pypi:>=1.0.0,<2.0.0`,
            },
          },
        },
        async ({path, run}) => {
          const registryUrl = await tests.startPackageServer();
          await configurePythonIsland(path, registryUrl);
          await writePythonProject(path, editableBackend);
          await writeLocalDependencyProject(path);

          await run(`install`);

          const {stdout} = await run(
            `python`,
            `-c`,
            [
              `import importlib.metadata, json, local_dependency, local_project, pypi_no_deps`,
              `print(json.dumps({`,
              `  "root": local_project.VALUE,`,
              `  "dependency": local_dependency.VALUE,`,
              `  "dependency_version": importlib.metadata.version("local-dependency"),`,
              `  "transitive": pypi_no_deps.VALUE,`,
              `}))`,
            ].join(`\n`),
            {cwd: workspacePath(path)},
          );

          expect(JSON.parse(stdout.trim())).toEqual({
            root: `installed-from-workspace`,
            dependency: `installed-from-local-dependency`,
            dependency_version: `4.5.6`,
            transitive: `1.1.0`,
          });
        },
      ),
    );

  });
});

function workspacePath(path: PortablePath) {
  return ppath.join(path, `packages/workspace-a` as PortablePath);
}

function entryPointPath() {
  return process.platform === `win32`
    ? `.venv/Scripts/local-project-cli.cmd`
    : `.venv/bin/local-project-cli`;
}
