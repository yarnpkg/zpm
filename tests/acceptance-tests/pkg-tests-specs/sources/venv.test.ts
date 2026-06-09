import {PortablePath, npath, xfs} from '@yarnpkg/fslib';
import {tests, yarn}              from 'pkg-tests-core';

async function configureVenvIsland(path: PortablePath, workspaces: Array<string>) {
  await yarn.writeConfiguration(path, {
    unstableIslands: {
      main: {
        workspaces,
        linker: `venv`,
      },
    },
  });
}

async function readLockfile(path: PortablePath) {
  const raw = await xfs.readFilePromise(`${path}/yarn.lock` as PortablePath, `utf8`);
  return JSON.parse(raw);
}

function currentSupportedTarget(version: string) {
  return {
    os: process.platform,
    cpu: process.arch,
    python: {
      version,
    },
  };
}

describe(`Venv tests`, () => {
  test(
    `it should make bin entries available from yarn python in venv island workspaces`,
    makeTemporaryMonorepoEnv(
      {
        workspaces: [`packages/*`],
      },
      {
        [`packages/workspace-a`]: {
          name: `workspace-a`,
          version: `1.0.0`,
          dependencies: {
            [`pypi-entry-points`]: `pypi:1.0.0`,
          },
        },
      },
      async ({path, run}) => {
        const registryUrl = await tests.startPackageServer();

        await configureVenvIsland(path, [`workspace-a`]);
        await run(`install`, {
          env: {
            ZPM_PYPI_REGISTRY: registryUrl,
          },
        });

        const {stdout} = await run(
          `python`,
          `-c`,
          [
            `import json, shutil, subprocess`,
            `binary = shutil.which("pypi-entry-points")`,
            `result = None`,
            `if binary is not None:`,
            `  result = subprocess.run(["pypi-entry-points", "binary-executed"], check=False, capture_output=True, text=True)`,
            `print(json.dumps({`,
            `  "binary": binary,`,
            `  "code": None if result is None else result.returncode,`,
            `  "stdout": "" if result is None else result.stdout.strip(),`,
            `  "stderr": "" if result is None else result.stderr.strip(),`,
            `}))`,
          ].join(`\n`),
          {cwd: `${path}/packages/workspace-a` as PortablePath},
        );

        const data = JSON.parse(stdout.trim()) as {
          binary: string | null;
          code: number | null;
          stdout: string;
          stderr: string;
        };

        expect(data.binary).not.toBe(null);
        expect(data).toMatchObject({
          code: 0,
          stdout: `binary-executed`,
          stderr: ``,
        });
      },
    ),
  );

  test(
    `it should resolve marker-conditioned PyPI forks and link the selected Python version`,
    makeTemporaryMonorepoEnv(
      {
        workspaces: [`packages/*`],
      },
      {
        [`packages/workspace-a`]: {
          name: `workspace-a`,
          version: `1.0.0`,
          dependencies: {
            [`pypi-marker-split`]: `pypi:1.0.0`,
          },
        },
      },
      async ({path, run}) => {
        const registryUrl = await tests.startPackageServer();

        await yarn.writeConfiguration(path, {
          supportedTargets: [
            currentSupportedTarget(`3.11`),
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

        const lockfile = await readLockfile(path);
        const forks = Object.values(lockfile.islands.main.forks) as Array<any>;
        expect(forks).toHaveLength(2);

        const forkResolutions = forks.flatMap(fork => {
          return Object.values(fork.entries ?? {}).map((entry: any) => entry.resolution.resolution);
        });

        expect(forkResolutions.some((resolution: string) => resolution.includes(`pypi-no-deps`) && resolution.includes(`1.0.0`))).toBe(true);
        expect(forkResolutions.some((resolution: string) => resolution.includes(`pypi-no-deps`) && resolution.includes(`1.1.0`))).toBe(true);

        const linkedSelectedDistInfo = npath.toPortablePath(`${path}/packages/workspace-a/.venv/lib/site-packages/pypi-no-deps/pypi_no_deps-1.1.0.dist-info/METADATA`);
        const linkedOtherDistInfo = npath.toPortablePath(`${path}/packages/workspace-a/.venv/lib/site-packages/pypi-no-deps/pypi_no_deps-1.0.0.dist-info/METADATA`);

        await expect(xfs.existsPromise(linkedSelectedDistInfo)).resolves.toBe(true);
        await expect(xfs.existsPromise(linkedOtherDistInfo)).resolves.toBe(false);
      },
    ),
  );

  test(
    `it should resolve PyPI releases compatible with each Python target`,
    makeTemporaryMonorepoEnv(
      {
        workspaces: [`packages/*`],
      },
      {
        [`packages/workspace-a`]: {
          name: `workspace-a`,
          version: `1.0.0`,
          dependencies: {
            [`pypi-python-version-split`]: `pypi:>=1.0.0`,
          },
        },
      },
      async ({path, run}) => {
        const registryUrl = await tests.startPackageServer();

        await yarn.writeConfiguration(path, {
          supportedTargets: [
            currentSupportedTarget(`3.11`),
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

        const lockfile = await readLockfile(path);
        const forks = Object.values(lockfile.islands.main.forks) as Array<any>;
        expect(forks).toHaveLength(2);

        const forkResolutions = forks.flatMap(fork => {
          return Object.values(fork.entries ?? {}).map((entry: any) => entry.resolution.resolution);
        });

        expect(forkResolutions.some((resolution: string) => resolution.includes(`pypi-python-version-split`) && resolution.includes(`1.0.0`))).toBe(true);
        expect(forkResolutions.some((resolution: string) => resolution.includes(`pypi-python-version-split`) && resolution.includes(`1.1.0`))).toBe(true);

        const linkedSelectedDistInfo = npath.toPortablePath(`${path}/packages/workspace-a/.venv/lib/site-packages/pypi-python-version-split/pypi_python_version_split-1.1.0.dist-info/METADATA`);
        const linkedOtherDistInfo = npath.toPortablePath(`${path}/packages/workspace-a/.venv/lib/site-packages/pypi-python-version-split/pypi_python_version_split-1.0.0.dist-info/METADATA`);

        await expect(xfs.existsPromise(linkedSelectedDistInfo)).resolves.toBe(true);
        await expect(xfs.existsPromise(linkedOtherDistInfo)).resolves.toBe(false);
      },
    ),
  );
});
