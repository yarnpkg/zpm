describe(`Protocols`, () => {
  describe(`Semver`, () => {
    test(
      `it should allow prefixing semver ranges with "v"`,
      makeTemporaryEnv(
        {
          dependencies: {[`no-deps`]: `v1.0.0`},
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
          dependencies: {[`no-deps-build-metadata`]: `1.0.0+123`},
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
      `it should allow installing packages whose registry metadata lists versions with components above 2^32`,
      makeTemporaryEnv(
        {
          dependencies: {[`large-version`]: `1.0.0`},
        },
        async ({path, run, source}) => {
          await run(`install`);

          await expect(source(`require('large-version')`)).resolves.toMatchObject({
            name: `large-version`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `it should allow semver ranges and versions with components above 2^32`,
      makeTemporaryEnv(
        {
          dependencies: {[`one-large-range-dep`]: `1.0.0`},
        },
        async ({path, run, source}) => {
          await run(`install`);

          await expect(source(`require('one-large-range-dep')`)).resolves.toMatchObject({
            name: `one-large-range-dep`,
            version: `1.0.0`,
            dependencies: {
              [`large-version`]: {
                name: `large-version`,
                version: `1.99999999204721.0`,
              },
            },
          });
        },
      ),
    );

    test(
      `it should treat an empty range as "*"`,
      makeTemporaryEnv(
        {
          dependencies: {[`no-deps`]: ``},
        },
        async ({path, run, source}) => {
          await run(`install`);

          await expect(source(`require('no-deps')`)).resolves.toMatchObject({
            name: `no-deps`,
            version: `2.0.0`,
          });

          await run(`install`, `--immutable`);
        },
      ),
    );

    test(
      `it should treat an empty range as "*" in transitive dependencies`,
      makeTemporaryEnv(
        {
          dependencies: {[`one-empty-range-dep`]: `1.0.0`},
        },
        async ({path, run, source}) => {
          await run(`install`);

          await expect(source(`require('one-empty-range-dep')`)).resolves.toMatchObject({
            name: `one-empty-range-dep`,
            version: `1.0.0`,
            dependencies: {
              [`no-deps`]: {
                name: `no-deps`,
                version: `2.0.0`,
              },
            },
          });

          await run(`install`, `--immutable`);
        },
      ),
    );
  });
});
