import {PortablePath} from '@yarnpkg/fslib';
import {tests, yarn}  from 'pkg-tests-core';

async function configureVenvIsland(path: PortablePath, workspaces: Array<string>) {
  await yarn.writeConfiguration(path, {
    pypiRegistryServer: await tests.startPackageServer(),
    unstableIslands: {
      main: {
        workspaces,
        linker: `venv`,
      },
    },
  });
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
        await configureVenvIsland(path, [`workspace-a`]);
        await run(`install`);

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
});
