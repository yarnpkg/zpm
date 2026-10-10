import {ppath, xfs} from '@yarnpkg/fslib';

const {
  fs: {writeFile, writeJson},
  tests: {getPackageArchivePath, getPackageDirectoryPath},
} = require(`pkg-tests-core`);

describe(`Features`, () => {
  describe(`Resolutions`, () => {
    describe.each([`-`, null])(`Removal resolutions (%p)`, removal => {
      test(
        `it should remove only matching dependencies and refresh when restored`,
        makeTemporaryEnv(
          {
            dependencies: {
              [`two-range-deps`]: `1.0.0`,
              [`one-fixed-dep`]: `1.0.0`,
            },
            resolutions: {
              [`two-range-deps/no-deps`]: removal,
            },
          },
          async ({path, run, source}) => {
            await run(`install`);

            await expect(source(`require('two-range-deps')`)).rejects.toMatchObject({
              externalException: {code: `MODULE_NOT_FOUND`},
            });

            await expect(
              source(`require(require.resolve('@types/is-number/package.json', {paths: [require.resolve('two-range-deps/package.json')]})).version`),
            ).resolves.toEqual(`2.0.0`);

            await expect(source(`require('one-fixed-dep')`)).resolves.toMatchObject({
              dependencies: {[`no-deps`]: {version: `1.0.0`}},
            });

            await run(`install`, `--immutable`);

            const manifestPath = ppath.join(path, `package.json`);
            const manifest = await xfs.readJsonPromise(manifestPath);
            manifest.resolutions[`two-range-deps/no-deps`] = `2.0.0`;
            await xfs.writeJsonPromise(manifestPath, manifest);

            await run(`install`);

            await expect(source(`require('two-range-deps')`)).resolves.toMatchObject({
              dependencies: {[`no-deps`]: {version: `2.0.0`}},
            });
          },
        ),
      );

      test(
        `it should remove only matching peers without synthesizing their types peers`,
        makeTemporaryEnv(
          {
            dependencies: {
              [`peer-consumer`]: `portal:./peer-consumer`,
              [`no-deps`]: `1.0.0`,
              [`is-number`]: `1.0.0`,
              [`@types/no-deps`]: `1.0.0`,
            },
            resolutions: {
              [`peer-consumer/no-deps`]: removal,
            },
          },
          async ({path, run, source}) => {
            await writeJson(`${path}/peer-consumer/package.json`, {
              name: `peer-consumer`,
              version: `1.0.0`,
              peerDependencies: {
                [`no-deps`]: `*`,
                [`is-number`]: `*`,
              },
            });

            await run(`install`);

            await expect(
              source(`{
                const pnp = require('pnpapi');
                const locator = pnp.findPackageLocator(require.resolve('peer-consumer/package.json'));
                return [...pnp.getPackageInformation(locator).packagePeers];
              }`),
            ).resolves.toEqual([`@types/is-number`, `is-number`]);
          },
        ),
      );

      test(
        `it should remove synthesized types peers while keeping the original peers`,
        makeTemporaryEnv(
          {
            dependencies: {
              [`peer-deps`]: `1.0.0`,
              [`no-deps`]: `1.0.0`,
              [`@types/no-deps`]: `1.0.0`,
            },
            resolutions: {
              [`@types/no-deps`]: removal,
            },
          },
          async ({path, run, source}) => {
            await run(`install`);

            await expect(
              source(`{
                const pnp = require('pnpapi');
                const locator = pnp.findPackageLocator(require.resolve('peer-deps/package.json'));
                return [...pnp.getPackageInformation(locator).packagePeers];
              }`),
            ).resolves.toEqual([`no-deps`]);

            await expect(source(`require('peer-deps')`)).resolves.toMatchObject({
              peerDependencies: {[`no-deps`]: {version: `1.0.0`}},
            });
          },
        ),
      );
    });

    test(
      `it should support overriding a packages with another`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`one-fixed-dep`]: `1.0.0`,
          },
          resolutions: {
            [`no-deps`]: `2.0.0`,
          },
        },
        async ({path, run, source}) => {
          await run(`install`);

          await expect(source(`require('one-fixed-dep')`)).resolves.toMatchObject({
            name: `one-fixed-dep`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: {
                name: `no-deps`,
                version: `2.0.0`,
              },
            },
          });
        },
      ),
    );

    test(
      `it should support overriding a packages with another, but only if it's the dependency of a specific other package`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`one-fixed-dep`]: `1.0.0`,
            [`one-range-dep`]: `1.0.0`,
          },
          resolutions: {
            [`one-range-dep/no-deps`]: `2.0.0`,
          },
        },
        async ({path, run, source}) => {
          await run(`install`);

          await expect(source(`require('one-fixed-dep')`)).resolves.toMatchObject({
            name: `one-fixed-dep`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: {
                name: `no-deps`,
                version: `1.0.0`,
              },
            },
          });

          await expect(source(`require('one-range-dep')`)).resolves.toMatchObject({
            name: `one-range-dep`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: {
                name: `no-deps`,
                version: `2.0.0`,
              },
            },
          });
        },
      ),
    );

    test(
      `it should support overriding a packages with another, but only if it originally resolved to a specific reference`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`one-fixed-dep`]: `1.0.0`,
            [`no-deps`]: `1.1.0`,
          },
          resolutions: {
            [`no-deps@1.0.0`]: `2.0.0`,
          },
        },
        async ({path, run, source}) => {
          await run(`install`);

          await expect(source(`require('no-deps')`)).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.1.0`,
          });

          await expect(source(`require('one-fixed-dep')`)).resolves.toMatchObject({
            name: `one-fixed-dep`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: {
                name: `no-deps`,
                version: `2.0.0`,
              },
            },
          });
        },
      ),
    );

    test(
      `it should support overriding a packages with another, but only if its parent package has a specific version`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`one-fixed-dep`]: `1.0.0`,
          },
          resolutions: {
            [`one-fixed-dep@1.0.0/no-deps`]: `1.0.1`,
          },
        },
        async ({path, run, source}) => {
          await run(`install`);

          await expect(source(`require('one-fixed-dep')`)).resolves.toMatchObject({
            name: `one-fixed-dep`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: {
                name: `no-deps`,
                version: `1.0.1`,
              },
            },
          });

          await xfs.writeJsonPromise(ppath.join(path, `package.json`), {
            dependencies: {
              [`one-fixed-dep`]: `2.0.0`,
            },
            resolutions: {
              [`one-fixed-dep@1.0.0/no-deps`]: `1.0.1`,
            },
          });

          await run(`install`);

          await expect(source(`require('one-fixed-dep')`)).resolves.toMatchObject({
            name: `one-fixed-dep`,
            version: `2.0.0`,
            dependencies: {
              [`no-deps`]: {
                name: `no-deps`,
                version: `2.0.0`,
              },
            },
          });
        },
      ),
    );

    test(
      `it should support overriding packages with portals`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`one-fixed-dep`]: `1.0.0`,
          },
          resolutions: {
            [`no-deps`]: `portal:./my-package`,
          },
        },
        async ({path, run, source}) => {
          await writeFile(`${path}/my-package/index.js`, `module.exports = 42;\n`);
          await writeJson(`${path}/my-package/package.json`, {
            name: `no-deps`,
            version: `42.0.0`,
          });

          await run(`install`);

          await expect(source(`require('one-fixed-dep')`)).resolves.toMatchObject({
            name: `one-fixed-dep`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: 42,
            },
          });
        },
      ),
    );

    test(
      `it should warn when legacy glob syntax is used`,
      makeTemporaryEnv(
        {
          resolutions: {
            [`**/no-deps`]: `1.2.0`,
          },
        },
        async ({path, run, source}) => {
          const {stdout} = await run(`install`);

          expect(stdout).toContain(`Legacy glob syntax found in resolutions`);
        },
      ),
    );

    test(
      `it should support overriding packages with tarballs`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`one-fixed-dep`]: `1.0.0`,
          },
          resolutions: {
            [`no-deps`]: getPackageArchivePath(`no-deps`, `2.0.0`),
          },
        },
        async ({path, run, source}) => {
          await run(`install`);

          await expect(source(`require('one-fixed-dep')`)).resolves.toMatchObject({
            name: `one-fixed-dep`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: {
                name: `no-deps`,
                version: `2.0.0`,
              },
            },
          });
        },
      ),
    );

    test(
      `it should support overriding packages with directories`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`one-fixed-dep`]: `1.0.0`,
          },
          resolutions: {
            [`no-deps`]: getPackageDirectoryPath(`no-deps`, `2.0.0`),
          },
        },
        async ({path, run, source}) => {
          await run(`install`);

          await expect(source(`require('one-fixed-dep')`)).resolves.toMatchObject({
            name: `one-fixed-dep`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: {
                name: `no-deps`,
                version: `2.0.0`,
              },
            },
          });
        },
      ),
    );

    test(
      `it should detect that a resolution entry got removed`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`one-fixed-dep`]: `1.0.0`,
          },
          resolutions: {
            [`no-deps`]: getPackageDirectoryPath(`no-deps`, `2.0.0`),
          },
        },
        async ({path, run, source}) => {
          await run(`install`);

          await expect(source(`require('one-fixed-dep')`)).resolves.toMatchObject({
            name: `one-fixed-dep`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: {
                name: `no-deps`,
                version: `2.0.0`,
              },
            },
          });

          await xfs.writeJsonPromise(`${path}/package.json`, {
            dependencies: {
              [`one-fixed-dep`]: `1.0.0`,
            },
          });

          await run(`install`);

          await expect(source(`require('one-fixed-dep')`)).resolves.toMatchObject({
            name: `one-fixed-dep`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: {
                name: `no-deps`,
                version: `1.0.0`,
              },
            },
          });
        },
      ),
    );
  });
});
