import {xfs} from '@yarnpkg/fslib';

describe(`Protocols`, () => {
  describe(`npm:`, () => {
    test(
      `it should allow renaming packages`,
      makeTemporaryEnv(
        {
          dependencies: {[`no-deps`]: `npm:one-fixed-dep@1.0.0`},
        },
        async ({path, run, source}) => {
          await run(`install`);

          await expect(source(`require('no-deps')`)).resolves.toMatchObject({
            name: `one-fixed-dep`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `it should allow prefixing semver ranges with "v"`,
      makeTemporaryEnv(
        {
          dependencies: {[`no-deps`]: `npm:v1.0.0`},
        },
        async ({path, run, source}) => {
          await run(`install`);

          await expect(source(`require('no-deps')`)).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `it should allow semver ranges with build metadata`,
      makeTemporaryEnv(
        {
          dependencies: {[`no-deps-build-metadata`]: `npm:1.0.0+123`},
        },
        async ({path, run, source}) => {
          await run(`install`);

          await expect(source(`require('no-deps-build-metadata')`)).resolves.toMatchObject({
            name: `no-deps-build-metadata`,
            version: `1.0.0+123`,
          });
        },
      ),
    );

    test(
      `it should allow fetching packages that have an unconventional url (semver)`,
      makeTemporaryEnv(
        {
          dependencies: {[`unconventional-tarball`]: `1.0.0`},
        },
        async ({path, run, source}) => {
          await run(`install`);

          await expect(source(`require('unconventional-tarball')`)).resolves.toMatchObject({
            name: `unconventional-tarball`,
            version: `1.0.0`,
          });

          await xfs.removePromise(`${path}/.yarn`);
          await run(`install`);
        },
      ),
    );

    test(
      `it should allow fetching packages that have an unconventional url (tag)`,
      makeTemporaryEnv(
        {
          dependencies: {[`unconventional-tarball`]: `latest`},
        },
        async ({path, run, source}) => {
          await run(`install`);

          await expect(source(`require('unconventional-tarball')`)).resolves.toMatchObject({
            name: `unconventional-tarball`,
            version: `2.0.0`,
          });

          await xfs.removePromise(`${path}/.yarn`);
          await run(`install`);
        },
      ),
    );

    test(
      `it should allow fetching packages that have an unconventional url without specifying version`,
      makeTemporaryEnv({}, async ({run, source}) => {
        await run(`add`, `unconventional-tarball`);

        await expect(source(`require('unconventional-tarball')`)).resolves.toMatchObject({
          name: `unconventional-tarball`,
          version: `2.0.0`,
        });
      }),
    );

    test(
      `it should allow fetching packages that have an unconventional url (fragment)`,
      makeTemporaryEnv(
        {
          dependencies: {[`unconventional-tarball`]: `https://registry.example.org/unconventional-tarball/tralala/unconventional-tarball-1.0.0.tgz#fragment`},
        },
        async ({path, run, source, registryUrl}) => {
          await run(`install`);

          await expect(source(`require('unconventional-tarball')`)).resolves.toMatchObject({
            name: `unconventional-tarball`,
            version: `1.0.0`,
          });

          await xfs.removePromise(`${path}/.yarn`);
          await run(`install`);
        },
      ),
    );

    test(
      `it should allow fetching packages that have an unconventional url (query)`,
      makeTemporaryEnv(
        {
          dependencies: {[`unconventional-tarball`]: `https://registry.example.org/unconventional-tarball/tralala/unconventional-tarball-1.0.0.tgz?auth=1234`},
        },
        async ({path, run, source, registryUrl}) => {
          await run(`install`);

          await expect(source(`require('unconventional-tarball')`)).resolves.toMatchObject({
            name: `unconventional-tarball`,
            version: `1.0.0`,
          });

          await xfs.removePromise(`${path}/.yarn`);
          await run(`install`);
        },
      ),
    );

    test(
      `it should allow aliasing packages that have an unconventional url`,
      makeTemporaryEnv(
        {
          dependencies: {[`aliased-unconventional`]: `npm:unconventional-tarball@1.0.0`},
        },
        async ({path, run, source}) => {
          await run(`install`);

          await expect(source(`require('aliased-unconventional')`)).resolves.toMatchObject({
            name: `unconventional-tarball`,
            version: `1.0.0`,
          });

          await xfs.removePromise(`${path}/.yarn`);
          await run(`install`);
        },
      ),
    );

    test(
      `it should resolve a "*" range to a prerelease when the package only has prereleases`,
      makeTemporaryEnv(
        {
          dependencies: {[`prerelease-only`]: `*`},
        },
        async ({run, source}) => {
          await run(`install`);

          await expect(source(`require('prerelease-only')`)).resolves.toMatchObject({
            name: `prerelease-only`,
            version: `1.0.0-rc.2`,
          });
        },
      ),
    );

    test(
      `it should resolve a "*" range to the stable version when the package has one`,
      makeTemporaryEnv(
        {
          dependencies: {[`no-deps-tags`]: `*`},
        },
        async ({run, source}) => {
          await run(`install`);

          await expect(source(`require('no-deps-tags')`)).resolves.toMatchObject({
            name: `no-deps-tags`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `it should pass --check-resolutions for a "*" range that only has prereleases`,
      makeTemporaryEnv(
        {
          dependencies: {[`prerelease-only`]: `*`},
        },
        async ({run}) => {
          await run(`install`);
          await run(`install`, `--check-resolutions`);
        },
      ),
    );

    // The `invalid-versions` package is published as 1.0.0 and 2.0.0, but its
    // registry metadata also lists versions that aren't valid semver: `3.0`
    // and `01.5.0` (only accepted by node-semver's loose mode), `4.0.0.1`, and
    // `9007199254740992.0.0` (above Number.MAX_SAFE_INTEGER). Its `legacy` tag
    // points to `3.0`.
    describe(`registries listing invalid versions`, () => {
      test(
        `it should resolve an exact version`,
        makeTemporaryEnv(
          {
            dependencies: {[`invalid-versions`]: `1.0.0`},
          },
          async ({run, source}) => {
            await run(`install`);

            await expect(source(`require('invalid-versions')`)).resolves.toMatchObject({
              name: `invalid-versions`,
              version: `1.0.0`,
            });
          },
        ),
      );

      test(
        `it should resolve a range to the highest valid version`,
        makeTemporaryEnv(
          {
            dependencies: {[`invalid-versions`]: `^1.0.0`},
          },
          async ({run, source}) => {
            await run(`install`);

            await expect(source(`require('invalid-versions')`)).resolves.toMatchObject({
              name: `invalid-versions`,
              version: `1.0.0`,
            });
          },
        ),
      );

      test(
        `it should resolve a tag pointing to a valid version`,
        makeTemporaryEnv(
          {
            dependencies: {[`invalid-versions`]: `latest`},
          },
          async ({run, source}) => {
            await run(`install`);

            await expect(source(`require('invalid-versions')`)).resolves.toMatchObject({
              name: `invalid-versions`,
              version: `2.0.0`,
            });
          },
        ),
      );

      test(
        `it should add the highest valid version`,
        makeTemporaryEnv({}, async ({path, run, source}) => {
          await run(`add`, `invalid-versions`);

          await expect(xfs.readJsonPromise(`${path}/package.json`)).resolves.toMatchObject({
            dependencies: {
              [`invalid-versions`]: `^2.0.0`,
            },
          });

          await expect(source(`require('invalid-versions')`)).resolves.toMatchObject({
            name: `invalid-versions`,
            version: `2.0.0`,
          });
        }),
      );

      test(
        `it should report a clear error when a tag points to an invalid version`,
        makeTemporaryEnv(
          {
            dependencies: {[`invalid-versions`]: `legacy`},
          },
          async ({run}) => {
            await expect(run(`install`)).rejects.toThrow(/The `legacy` tag of invalid-versions points to an invalid semver version \(3\.0\)/);
          },
        ),
      );

      test(
        `it should report a clear error when requesting an invalid version`,
        makeTemporaryEnv(
          {
            dependencies: {[`invalid-versions`]: `3.0`},
          },
          async ({run}) => {
            await expect(run(`install`)).rejects.toThrow(/The registry lists 3\.0 as a version of invalid-versions, but it isn't a valid semver version/);
          },
        ),
      );
    });
  });
});
