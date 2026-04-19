import {PortablePath, npath, xfs} from '@yarnpkg/fslib';
import {yarn}                     from 'pkg-tests-core';

const {
  tests: {getPackageArchivePath, getPackageHttpArchivePath, getPackageDirectoryPath},
} = require(`pkg-tests-core`);

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

async function runPythonJson(run: any, cwd: PortablePath, script: string) {
  const {stdout} = await run(`python`, `-c`, script, {cwd});
  return JSON.parse(stdout.trim());
}

describe(`Features`, () => {
  describe(`Venv linker`, () => {
    test(
      `it should configure VIRTUAL_ENV and PYTHONPATH when using yarn python from a venv island workspace`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/island-ws`]: {
            name: `island-ws`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
          },
          [`packages/pnp-ws`]: {
            name: `pnp-ws`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `2.0.0`,
            },
          },
        },
        async ({path, run}) => {
          await configureVenvIsland(path, [`island-ws`]);

          await run(`install`);

          const pythonIntrospection = [
            `import json, os, sys`,
            `print(json.dumps({`,
            `  "virtual_env": os.environ.get("VIRTUAL_ENV"),`,
            `  "pythonpath": os.environ.get("PYTHONPATH"),`,
            `  "has_site_packages": any(p.endswith("/.venv/lib/site-packages") for p in sys.path),`,
            `}))`,
          ].join(`\n`);

          const {stdout} = await run(
            `python`,
            `-c`,
            pythonIntrospection,
            {cwd: `${path}/packages/island-ws` as PortablePath},
          );

          const data = JSON.parse(stdout.trim()) as {
            virtual_env: string | null;
            pythonpath: string | null;
            has_site_packages: boolean;
          };

          expect(data.virtual_env).toContain(`/packages/island-ws/.venv`);
          expect(data.pythonpath).toContain(`/packages/island-ws/.venv/lib/site-packages`);
          expect(data.has_site_packages).toBe(true);

          const {stdout: versionStdout} = await run(
            `python`,
            `-c`,
            [
              `import json, os, pathlib`,
              `manifest = pathlib.Path(os.environ["VIRTUAL_ENV"]) / "lib" / "site-packages" / "no-deps" / "package.json"`,
              `print(json.loads(manifest.read_text())["version"])`,
            ].join(`\n`),
            {cwd: `${path}/packages/island-ws` as PortablePath},
          );

          expect(versionStdout.trim()).toBe(`1.0.0`);

          const {stdout: nonIslandStdout} = await run(
            `python`,
            `-c`,
            [
              `import json, os`,
              `print(json.dumps({"virtual_env": os.environ.get("VIRTUAL_ENV")}))`,
            ].join(`\n`),
            {cwd: `${path}/packages/pnp-ws` as PortablePath},
          );

          const nonIslandData = JSON.parse(nonIslandStdout.trim()) as {
            virtual_env: string | null;
          };

          expect(nonIslandData.virtual_env).toBe(null);
        },
      ),
    );

    test(
      `it should install island dependencies inside a workspace venv`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`one-fixed-dep`]: `1.0.0`,
            },
          },
        },
        async ({path, run}) => {
          await configureVenvIsland(path, [`workspace-a`]);

          await run(`install`);

          const directPackageJsonPath = npath.toPortablePath(`${path}/packages/workspace-a/.venv/lib/site-packages/one-fixed-dep/package.json`);
          const transitivePackageJsonPath = npath.toPortablePath(`${path}/packages/workspace-a/.venv/lib/site-packages/no-deps/package.json`);

          await expect(xfs.existsPromise(directPackageJsonPath)).resolves.toBe(true);
          await expect(xfs.existsPromise(transitivePackageJsonPath)).resolves.toBe(true);

          const directManifest = await xfs.readJsonPromise(directPackageJsonPath) as Record<string, string>;
          const transitiveManifest = await xfs.readJsonPromise(transitivePackageJsonPath) as Record<string, string>;

          expect(directManifest.version).toBe(`1.0.0`);
          expect(transitiveManifest.version).toBe(`1.0.0`);
        },
      ),
    );

    test(
      `it should allow mixing a venv island with regular PnP workspaces`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/island-ws`]: {
            name: `island-ws`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
          },
          [`packages/pnp-ws`]: {
            name: `pnp-ws`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `2.0.0`,
            },
          },
        },
        async ({path, run, source}) => {
          await configureVenvIsland(path, [`island-ws`]);

          await run(`install`);

          const islandPackageJsonPath = npath.toPortablePath(`${path}/packages/island-ws/.venv/lib/site-packages/no-deps/package.json`);
          const islandManifest = await xfs.readJsonPromise(islandPackageJsonPath) as Record<string, string>;

          expect(islandManifest.version).toBe(`1.0.0`);

          await expect(
            source(`require('no-deps')`, {cwd: `${path}/packages/pnp-ws` as PortablePath}),
          ).resolves.toMatchObject({
            name: `no-deps`,
            version: `2.0.0`,
          });

          await expect(
            xfs.existsPromise(npath.toPortablePath(`${path}/packages/pnp-ws/.venv`)),
          ).resolves.toBe(false);
        },
      ),
    );

    describe(`Python basic parity`, () => {
      test(
        `it should correctly install a single dependency that contains no sub-dependencies`,
        makeTemporaryMonorepoEnv(
          {
            workspaces: [`packages/*`],
          },
          {
            [`packages/workspace-a`]: {
              name: `workspace-a`,
              version: `1.0.0`,
              dependencies: {
                [`pynodeps`]: `1.0.0`,
              },
            },
          },
          async ({path, run}) => {
            await configureVenvIsland(path, [`workspace-a`]);
            await run(`install`);

            await expect(runPythonJson(run, `${path}/packages/workspace-a` as PortablePath, [
              `import json, pynodeps`,
              `print(json.dumps(pynodeps.to_dict()))`,
            ].join(`\n`))).resolves.toMatchObject({
              name: `pynodeps`,
              version: `1.0.0`,
            });
          },
        ),
      );

      test(
        `it should correctly install a dependency that itself contains a fixed dependency`,
        makeTemporaryMonorepoEnv(
          {
            workspaces: [`packages/*`],
          },
          {
            [`packages/workspace-a`]: {
              name: `workspace-a`,
              version: `1.0.0`,
              dependencies: {
                [`pyonefixeddep`]: `1.0.0`,
              },
            },
          },
          async ({path, run}) => {
            await configureVenvIsland(path, [`workspace-a`]);
            await run(`install`);

            await expect(runPythonJson(run, `${path}/packages/workspace-a` as PortablePath, [
              `import json, pyonefixeddep`,
              `print(json.dumps(pyonefixeddep.to_dict()))`,
            ].join(`\n`))).resolves.toMatchObject({
              name: `pyonefixeddep`,
              version: `1.0.0`,
              dependencies: {
                pynodeps: {
                  name: `pynodeps`,
                  version: `1.0.0`,
                },
              },
            });
          },
        ),
      );

      test(
        `it should correctly install a dependency that itself contains a range dependency`,
        makeTemporaryMonorepoEnv(
          {
            workspaces: [`packages/*`],
          },
          {
            [`packages/workspace-a`]: {
              name: `workspace-a`,
              version: `1.0.0`,
              dependencies: {
                [`pyonerangedep`]: `1.0.0`,
              },
            },
          },
          async ({path, run}) => {
            await configureVenvIsland(path, [`workspace-a`]);
            await run(`install`);

            await expect(runPythonJson(run, `${path}/packages/workspace-a` as PortablePath, [
              `import json, pyonerangedep`,
              `print(json.dumps(pyonerangedep.to_dict()))`,
            ].join(`\n`))).resolves.toMatchObject({
              name: `pyonerangedep`,
              version: `1.0.0`,
              dependencies: {
                pynodeps: {
                  name: `pynodeps`,
                  version: `1.1.0`,
                },
              },
            });
          },
        ),
      );

      test(
        `it should install from archives on the filesystem`,
        makeTemporaryMonorepoEnv(
          {
            workspaces: [`packages/*`],
          },
          {
            [`packages/workspace-a`]: {
              name: `workspace-a`,
              version: `1.0.0`,
              dependencies: {
                [`pynodeps`]: getPackageArchivePath(`pynodeps`, `1.0.0`),
              },
            },
          },
          async ({path, run}) => {
            await configureVenvIsland(path, [`workspace-a`]);
            await run(`install`);

            await expect(runPythonJson(run, `${path}/packages/workspace-a` as PortablePath, [
              `import json, pynodeps`,
              `print(json.dumps(pynodeps.to_dict()))`,
            ].join(`\n`))).resolves.toMatchObject({
              name: `pynodeps`,
              version: `1.0.0`,
            });
          },
        ),
      );

      test(
        `it should install the dependencies of any dependency fetched from the filesystem`,
        makeTemporaryMonorepoEnv(
          {
            workspaces: [`packages/*`],
          },
          {
            [`packages/workspace-a`]: {
              name: `workspace-a`,
              version: `1.0.0`,
              dependencies: {
                [`pyonefixeddep`]: getPackageArchivePath(`pyonefixeddep`, `1.0.0`),
              },
            },
          },
          async ({path, run}) => {
            await configureVenvIsland(path, [`workspace-a`]);
            await run(`install`);

            await expect(runPythonJson(run, `${path}/packages/workspace-a` as PortablePath, [
              `import json, pyonefixeddep`,
              `print(json.dumps(pyonefixeddep.to_dict()))`,
            ].join(`\n`))).resolves.toMatchObject({
              name: `pyonefixeddep`,
              version: `1.0.0`,
              dependencies: {
                pynodeps: {
                  name: `pynodeps`,
                  version: `1.0.0`,
                },
              },
            });
          },
        ),
      );

      test(
        `it should install from files on the internet`,
        makeTemporaryMonorepoEnv(
          {
            workspaces: [`packages/*`],
          },
          {
            [`packages/workspace-a`]: {
              name: `workspace-a`,
              version: `1.0.0`,
              dependencies: {
                [`pynodeps`]: getPackageHttpArchivePath(`pynodeps`, `1.0.0`),
              },
            },
          },
          async ({path, run}) => {
            await configureVenvIsland(path, [`workspace-a`]);
            await run(`install`);

            await expect(runPythonJson(run, `${path}/packages/workspace-a` as PortablePath, [
              `import json, pynodeps`,
              `print(json.dumps(pynodeps.to_dict()))`,
            ].join(`\n`))).resolves.toMatchObject({
              name: `pynodeps`,
              version: `1.0.0`,
            });
          },
        ),
      );

      test(
        `it should install the dependencies of any dependency fetched from the internet`,
        makeTemporaryMonorepoEnv(
          {
            workspaces: [`packages/*`],
          },
          {
            [`packages/workspace-a`]: {
              name: `workspace-a`,
              version: `1.0.0`,
              dependencies: {
                [`pyonefixeddep`]: getPackageHttpArchivePath(`pyonefixeddep`, `1.0.0`),
              },
            },
          },
          async ({path, run}) => {
            await configureVenvIsland(path, [`workspace-a`]);
            await run(`install`);

            await expect(runPythonJson(run, `${path}/packages/workspace-a` as PortablePath, [
              `import json, pyonefixeddep`,
              `print(json.dumps(pyonefixeddep.to_dict()))`,
            ].join(`\n`))).resolves.toMatchObject({
              name: `pyonefixeddep`,
              version: `1.0.0`,
              dependencies: {
                pynodeps: {
                  name: `pynodeps`,
                  version: `1.0.0`,
                },
              },
            });
          },
        ),
      );

      test(
        `it should install from local directories`,
        makeTemporaryMonorepoEnv(
          {
            workspaces: [`packages/*`],
          },
          {
            [`packages/workspace-a`]: {
              name: `workspace-a`,
              version: `1.0.0`,
              dependencies: {
                [`pynodeps`]: getPackageDirectoryPath(`pynodeps`, `1.0.0`),
              },
            },
          },
          async ({path, run}) => {
            await configureVenvIsland(path, [`workspace-a`]);
            await run(`install`);

            await expect(runPythonJson(run, `${path}/packages/workspace-a` as PortablePath, [
              `import json, pynodeps`,
              `print(json.dumps(pynodeps.to_dict()))`,
            ].join(`\n`))).resolves.toMatchObject({
              name: `pynodeps`,
              version: `1.0.0`,
            });
          },
        ),
      );

      test(
        `it should install the dependencies of any dependency fetched from a local directory`,
        makeTemporaryMonorepoEnv(
          {
            workspaces: [`packages/*`],
          },
          {
            [`packages/workspace-a`]: {
              name: `workspace-a`,
              version: `1.0.0`,
              dependencies: {
                [`pyonefixeddep`]: getPackageDirectoryPath(`pyonefixeddep`, `1.0.0`),
              },
            },
          },
          async ({path, run}) => {
            await configureVenvIsland(path, [`workspace-a`]);
            await run(`install`);

            await expect(runPythonJson(run, `${path}/packages/workspace-a` as PortablePath, [
              `import json, pyonefixeddep`,
              `print(json.dumps(pyonefixeddep.to_dict()))`,
            ].join(`\n`))).resolves.toMatchObject({
              name: `pyonefixeddep`,
              version: `1.0.0`,
              dependencies: {
                pynodeps: {
                  name: `pynodeps`,
                  version: `1.0.0`,
                },
              },
            });
          },
        ),
      );

      test(
        `it should correctly create resolution mounting points when using the link protocol`,
        makeTemporaryMonorepoEnv(
          {
            workspaces: [`packages/*`],
          },
          {
            [`packages/workspace-a`]: {
              name: `workspace-a`,
              version: `1.0.0`,
              dependencies: {
                [`pylinkdep`]: (async () => `link:${await getPackageDirectoryPath(`pynodeps`, `1.0.0`)}`)(),
              },
            },
          },
          async ({path, run}) => {
            await configureVenvIsland(path, [`workspace-a`]);
            await run(`install`);

            await expect(runPythonJson(run, `${path}/packages/workspace-a` as PortablePath, [
              `import json, pylinkdep`,
              `print(json.dumps(pylinkdep.to_dict()))`,
            ].join(`\n`))).resolves.toMatchObject({
              name: `pynodeps`,
              version: `1.0.0`,
            });
          },
        ),
      );
    });

    test(
      `it should not allow using venv as the project nodeLinker`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`no-deps`]: `1.0.0`,
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            nodeLinker: `venv` as any,
          });

          await expect(run(`install`)).rejects.toThrow(/venv/i);
        },
      ),
    );
  });
});
