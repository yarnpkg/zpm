import {PortablePath, xfs}   from '@yarnpkg/fslib';
import {yarn, fs as fsUtils} from 'pkg-tests-core';

describe(`Features`, () => {
  describe(`Catalogs`, () => {
    test(
      `it should resolve dependencies from the default catalog during install`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`no-deps`]: `catalog:`,
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            catalog: {
              [`no-deps`]: `2.0.0`,
            },
          });

          await run(`install`);

          await expect(source(`require('no-deps')`)).resolves.toMatchObject({
            name: `no-deps`,
            version: `2.0.0`,
          });
        },
      ),
    );

    test(
      `it should resolve dependencies from named catalogs during install`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`no-deps`]: `catalog:react18`,
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            catalogs: {
              react18: {
                [`no-deps`]: `2.0.0`,
              },
            },
          });

          await run(`install`);

          await expect(source(`require('no-deps')`)).resolves.toMatchObject({
            name: `no-deps`,
            version: `2.0.0`,
          });
        },
      ),
    );

    test(
      `it should resolve scoped package dependencies from catalogs`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`@scoped/create-test-app`]: `catalog:`,
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            catalog: {
              [`@scoped/create-test-app`]: `1.0.0`,
            },
          });

          await run(`install`);

          // Verify that the scoped package was resolved from catalog
          const lockfile = await xfs.readFilePromise(`${path}/yarn.lock` as PortablePath, `utf8`);
          expect(lockfile).toMatch(/@scoped\/create-test-app@npm:1\.0\.0/);
        },
      ),
    );

    test(
      `it should support multiple catalog entries in the same project`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`no-deps`]: `catalog:`,
            [`one-fixed-dep`]: `catalog:react18`,
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            catalog: {
              [`no-deps`]: `2.0.0`,
            },
            catalogs: {
              react18: {
                [`one-fixed-dep`]: `1.0.0`,
              },
            },
          });

          await run(`install`);

          await expect(source(`require('no-deps')`)).resolves.toMatchObject({
            name: `no-deps`,
            version: `2.0.0`,
          });

          await expect(source(`require('one-fixed-dep')`)).resolves.toMatchObject({
            name: `one-fixed-dep`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `it should work with different dependency types (devDependencies, peerDependencies)`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`one-fixed-dep`]: `catalog:tools`,
          },
          devDependencies: {
            [`no-deps`]: `catalog:`,
          },
          peerDependencies: {
            [`no-deps`]: `catalog:`,
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            catalog: {
              [`no-deps`]: `2.0.0`,
            },
            catalogs: {
              tools: {
                [`one-fixed-dep`]: `1.0.0`,
              },
            },
          });

          await run(`install`);

          // Check that the lockfile contains the resolved versions
          const lockfile = await xfs.readFilePromise(`${path}/yarn.lock` as PortablePath, `utf8`);
          expect(lockfile).toMatch(/no-deps@npm:2\.0\.0/);
          expect(lockfile).toMatch(/one-fixed-dep@npm:1\.0\.0/);
        },
      ),
    );

    test(
      `it should replace catalog references with actual versions during pack`,
      makeTemporaryEnv(
        {
          name: `my-package`,
          version: `1.0.0`,
          dependencies: {
            [`no-deps`]: `catalog:`,
          },
          devDependencies: {
            [`one-fixed-dep`]: `catalog:dev`,
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            catalog: {
              [`no-deps`]: `^2.0.0`,
            },
            catalogs: {
              dev: {
                [`one-fixed-dep`]: `~1.0.0`,
              },
            },
          });

          await run(`install`);
          await run(`pack`);

          // Unpack the tarball and check the package.json content
          await fsUtils.unpackToDirectory(path, `${path}/package.tgz` as PortablePath);

          const packedManifest = await xfs.readJsonPromise(`${path}/package/package.json` as PortablePath);

          expect(packedManifest.dependencies[`no-deps`]).toBe(`^2.0.0`);
          expect(packedManifest.devDependencies[`one-fixed-dep`]).toBe(`~1.0.0`);
        },
      ),
    );

    test(
      `it should handle complex version ranges in catalogs`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`no-deps`]: `catalog:`,
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            catalog: {
              [`no-deps`]: `>=1.0.0 <3.0.0`,
            },
          });

          await run(`install`);

          // Should resolve to the highest compatible version (2.0.0)
          await expect(source(`require('no-deps')`)).resolves.toMatchObject({
            name: `no-deps`,
            version: `2.0.0`,
          });
        },
      ),
    );

    test(
      `it should throw an error when catalog is not found`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`no-deps`]: `catalog:nonexistent`,
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            catalog: {
              [`no-deps`]: `2.0.0`,
            },
          });

          await expect(run(`install`)).rejects.toThrow();
        },
      ),
    );

    test(
      `it should throw an error when catalog entry is not found`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`nonexistent-package`]: `catalog:`,
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            catalog: {
              [`no-deps`]: `2.0.0`,
            },
          });

          await expect(run(`install`)).rejects.toThrow();
        },
      ),
    );

    test(
      `it should throw an error when default catalog is empty`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`no-deps`]: `catalog:`,
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            catalog: {},
          });

          await expect(run(`install`)).rejects.toThrow();
        },
      ),
    );

    test(
      `it should throw an error when protocol in catalog isn't supported by any resolver`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`no-deps`]: `catalog:`,
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            catalog: {
              [`no-deps`]: `unknown-protocol:2.0.0`,
            },
          });

          await expect(run(`install`)).rejects.toThrow();
        },
      ),
    );

    test(
      `it should work with file: protocol ranges in catalogs`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`my-local-package`]: `catalog:`,
          },
        },
        async ({path, run, source}) => {
          await xfs.mkdirPromise(`${path}/local-package` as PortablePath, {recursive: true});
          await xfs.writeJsonPromise(`${path}/local-package/package.json` as PortablePath, {
            name: `my-local-package`,
            version: `1.0.0`,
          });

          await yarn.writeConfiguration(path, {
            catalog: {
              [`my-local-package`]: `file:./local-package`,
            },
          });

          await run(`install`);

          await expect(source(`require('my-local-package/package.json')`)).resolves.toMatchObject({
            name: `my-local-package`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `it should work in workspace environments`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          packages: {},
        },
        async ({path, run}) => {
          // Create workspace package
          await xfs.mkdirPromise(`${path}/packages/workspace-a` as PortablePath, {recursive: true});
          await xfs.writeJsonPromise(`${path}/packages/workspace-a/package.json` as PortablePath, {
            name: `workspace-a`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: `catalog:`,
            },
          });

          await yarn.writeConfiguration(path, {
            catalog: {
              [`no-deps`]: `2.0.0`,
            },
          });

          await run(`install`);

          // Verify that the workspace dependency was resolved from catalog
          const lockfile = await xfs.readFilePromise(`${path}/yarn.lock` as PortablePath, `utf8`);
          expect(lockfile).toMatch(/no-deps@npm:2\.0\.0/);
        },
      ),
    );

    test(
      `it should report a catalog entry that references itself`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`no-deps`]: `catalog:`,
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            catalog: {
              [`no-deps`]: `catalog:`,
            },
          });

          // Expanding the entry used to recurse until the stack overflowed,
          // which aborts the process rather than reporting anything
          await expect(run(`install`)).rejects.toThrow(/references itself/);
        },
      ),
    );

    test(
      `it should report catalog entries that reference each other`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`no-deps`]: `catalog:`,
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            catalogs: {
              default: {
                [`no-deps`]: `catalog:other`,
              },
              other: {
                [`no-deps`]: `catalog:`,
              },
            },
          });

          await expect(run(`install`)).rejects.toThrow(/references itself/);
        },
      ),
    );

    test(
      `it should work with catalog references in peer dependencies`,
      makeTemporaryEnv(
        {
          peerDependencies: {
            [`no-deps`]: `catalog:`,
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            catalog: {
              [`no-deps`]: `2.0.0`,
            },
          });

          await run(`install`);

          const manifest = await xfs.readJsonPromise(`${path}/package.json` as PortablePath);
          expect(manifest.peerDependencies[`no-deps`]).toBe(`catalog:`);
        },
      ),
    );

    test(
      `it should resolve catalog references in workspace peer dependencies`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          packages: {},
        },
        async ({path, run}) => {
          await xfs.mkdirPromise(`${path}/packages/my-lib` as PortablePath, {recursive: true});
          await xfs.writeJsonPromise(`${path}/packages/my-lib/package.json` as PortablePath, {
            name: `my-lib`,
            version: `1.0.0`,
            peerDependencies: {
              [`no-deps`]: `catalog:`,
            },
          });

          await xfs.mkdirPromise(`${path}/packages/my-app` as PortablePath, {recursive: true});
          await xfs.writeJsonPromise(`${path}/packages/my-app/package.json` as PortablePath, {
            name: `my-app`,
            version: `1.0.0`,
            dependencies: {
              [`my-lib`]: `workspace:*`,
              [`no-deps`]: `2.0.0`,
            },
          });

          await yarn.writeConfiguration(path, {
            catalog: {
              [`no-deps`]: `2.0.0`,
            },
          });

          await run(`install`);

          const lockfile = await xfs.readFilePromise(`${path}/yarn.lock` as PortablePath, `utf8`);

          expect(lockfile).toMatch(/no-deps@npm:2\.0\.0/);
        },
      ),
    );

    const makePeerCatalogWorkspaces = async (path: PortablePath, peerRange: string, providedVersion: string) => {
      await xfs.mkdirPromise(`${path}/packages/my-lib` as PortablePath, {recursive: true});
      await xfs.writeJsonPromise(`${path}/packages/my-lib/package.json` as PortablePath, {
        name: `my-lib`,
        version: `1.0.0`,
        peerDependencies: {
          [`no-deps`]: peerRange,
        },
      });

      await xfs.mkdirPromise(`${path}/packages/my-app` as PortablePath, {recursive: true});
      await xfs.writeJsonPromise(`${path}/packages/my-app/package.json` as PortablePath, {
        name: `my-app`,
        version: `1.0.0`,
        dependencies: {
          [`my-lib`]: `workspace:*`,
          [`no-deps`]: providedVersion,
        },
      });
    };

    test(
      `it should check workspace peer dependencies against the range from the default catalog`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          packages: {},
        },
        async ({path, run}) => {
          await makePeerCatalogWorkspaces(path, `catalog:`, `2.0.0`);

          await yarn.writeConfiguration(path, {
            catalog: {
              [`no-deps`]: `^1.0.0`,
            },
          });

          const {stdout: installOut} = await run(`install`);
          expect(installOut).toMatch(/Some peer dependencies are incorrectly met/);

          const {stdout} = await run(`explain`, `peer-requirements`);
          expect(stdout).toMatch(/✘ .* provides no-deps .* to my-lib/);
        },
      ),
    );

    test(
      `it should accept workspace peer dependencies satisfying the range from the catalog`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          packages: {},
        },
        async ({path, run}) => {
          await makePeerCatalogWorkspaces(path, `catalog:`, `2.0.0`);

          await yarn.writeConfiguration(path, {
            catalog: {
              [`no-deps`]: `^2.0.0`,
            },
          });

          const {stdout: installOut} = await run(`install`);
          expect(installOut).not.toMatch(/Some peer dependencies are incorrectly met/);

          const {stdout} = await run(`explain`, `peer-requirements`);
          expect(stdout).toMatch(/✓ .* provides no-deps .* to my-lib/);
        },
      ),
    );

    test(
      `it should check workspace peer dependencies against the range from a named catalog`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          packages: {},
        },
        async ({path, run}) => {
          await makePeerCatalogWorkspaces(path, `catalog:legacy`, `2.0.0`);

          await yarn.writeConfiguration(path, {
            catalog: {
              [`no-deps`]: `^2.0.0`,
            },
            catalogs: {
              legacy: {
                [`no-deps`]: `^1.0.0`,
              },
            },
          });

          const {stdout: installOut} = await run(`install`);
          expect(installOut).toMatch(/Some peer dependencies are incorrectly met/);

          const {stdout} = await run(`explain`, `peer-requirements`);
          expect(stdout).toMatch(/✘ .* provides no-deps .* to my-lib/);
        },
      ),
    );

    test(
      `it should throw an error when the catalog entry of a peer dependency is not found`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          packages: {},
        },
        async ({path, run}) => {
          await makePeerCatalogWorkspaces(path, `catalog:`, `2.0.0`);

          await yarn.writeConfiguration(path, {
            catalog: {
              [`one-fixed-dep`]: `1.0.0`,
            },
          });

          await expect(run(`install`)).rejects.toThrow(/The no-deps peer dependency of my-lib@workspace:my-lib references the default catalog, which doesn't have an entry for it/);
        },
      ),
    );

    test(
      `it should throw an error when the catalog of a peer dependency is not found`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          packages: {},
        },
        async ({path, run}) => {
          await makePeerCatalogWorkspaces(path, `catalog:nonexistent`, `2.0.0`);

          await yarn.writeConfiguration(path, {
            catalog: {
              [`no-deps`]: `2.0.0`,
            },
          });

          await expect(run(`install`)).rejects.toThrow(/The no-deps peer dependency of my-lib@workspace:my-lib references the nonexistent catalog, which doesn't exist/);
        },
      ),
    );

    test(
      `it should throw an error when the catalog entry of a peer dependency can't be used as a peer range`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          packages: {},
        },
        async ({path, run}) => {
          await makePeerCatalogWorkspaces(path, `catalog:`, `2.0.0`);

          for (const [range, pattern] of [
            [`link:./no-deps`, /The no-deps peer dependency of my-lib@workspace:my-lib references the default catalog, whose entry \(link:\.\/no-deps\) can't be used as a peer range/],
            [`npm:no-deps-bins@^1.0.0`, /The no-deps peer dependency of my-lib@workspace:my-lib references the default catalog, whose entry \(npm:no-deps-bins@\^1\.0\.0\) can't be used as a peer range/],
          ] as const) {
            await yarn.writeConfiguration(path, {
              catalog: {
                [`no-deps`]: range,
              },
            });

            await expect(run(`install`)).rejects.toThrow(pattern);
          }
        },
      ),
    );

    test(
      `it should keep catalog references when adding peer dependencies`,
      makeTemporaryEnv(
        {},
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            catalog: {
              [`no-deps`]: `^2.0.0`,
            },
            catalogs: {
              dev: {
                [`one-fixed-dep`]: `~1.0.0`,
              },
            },
          });

          await run(`add`, `-P`, `no-deps@catalog:`, `one-fixed-dep@catalog:dev`);

          const manifest = await xfs.readJsonPromise(`${path}/package.json` as PortablePath);
          expect(manifest.peerDependencies).toEqual({
            [`no-deps`]: `catalog:`,
            [`one-fixed-dep`]: `catalog:dev`,
          });
        },
      ),
    );

    test(
      `it should replace catalog references in peer dependencies during pack`,
      makeTemporaryEnv(
        {
          name: `my-package`,
          version: `1.0.0`,
          dependencies: {
            [`no-deps`]: `catalog:`,
          },
          peerDependencies: {
            [`no-deps`]: `catalog:`,
            [`one-fixed-dep`]: `catalog:dev`,
          },
        },
        async ({path, run}) => {
          await yarn.writeConfiguration(path, {
            catalog: {
              [`no-deps`]: `^2.0.0`,
            },
            catalogs: {
              dev: {
                [`one-fixed-dep`]: `npm:~1.0.0`,
              },
            },
          });

          await run(`install`);
          await run(`pack`);

          await fsUtils.unpackToDirectory(path, `${path}/package.tgz` as PortablePath);

          const packedManifest = await xfs.readJsonPromise(`${path}/package/package.json` as PortablePath);

          expect(packedManifest.dependencies[`no-deps`]).toBe(`^2.0.0`);
          expect(packedManifest.peerDependencies[`no-deps`]).toBe(`^2.0.0`);
          expect(packedManifest.peerDependencies[`one-fixed-dep`]).toBe(`~1.0.0`);

          // The workspace manifest itself is left untouched
          const manifest = await xfs.readJsonPromise(`${path}/package.json` as PortablePath);
          expect(manifest.peerDependencies[`no-deps`]).toBe(`catalog:`);
        },
      ),
    );

    test(
      `it should not use the project catalogs for the peer dependencies of registry packages`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`peer-deps-catalog`]: `1.0.0`,
            [`no-deps`]: `2.0.0`,
          },
        },
        async ({path, run, source}) => {
          // The project catalog doesn't contain any entry for no-deps; a
          // `catalog:` peer range coming from the registry must neither
          // look into it nor make the install fail
          await yarn.writeConfiguration(path, {
            catalog: {
              [`one-fixed-dep`]: `1.0.0`,
            },
          });

          const {stdout} = await run(`install`);
          expect(stdout).not.toMatch(/Some peer dependencies are incorrectly met/);

          await expect(source(`require('peer-deps-catalog')`)).resolves.toMatchObject({
            name: `peer-deps-catalog`,
            version: `1.0.0`,
            peerDependencies: {
              [`no-deps`]: {
                name: `no-deps`,
                version: `2.0.0`,
              },
            },
          });
        },
      ),
    );
  });
});
