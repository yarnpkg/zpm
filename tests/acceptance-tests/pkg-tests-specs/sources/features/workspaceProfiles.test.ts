import {yarn} from 'pkg-tests-core';

describe(`Features`, () => {
  describe(`Workspace Profiles`, () => {
    test(
      `it should install devDependencies from a profile when a workspace extends it`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            extends: [`typescript`],
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            workspaceProfiles: {
              typescript: {
                devDependencies: {
                  [`no-deps`]: `1.0.0`,
                },
              },
            },
          });

          await run(`install`);

          // The devDependency from the profile should be available in the workspace
          await expect(
            source(`require('no-deps')`, {cwd: `${path}/packages/workspace-a`}),
          ).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `it should merge multiple profiles when a workspace extends them`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            extends: [`typescript`, `testing`],
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            workspaceProfiles: {
              typescript: {
                devDependencies: {
                  [`no-deps`]: `1.0.0`,
                },
              },
              testing: {
                devDependencies: {
                  [`one-fixed-dep`]: `1.0.0`,
                },
              },
            },
          });

          await run(`install`);

          // Both profiles' devDependencies should be available
          await expect(
            source(`require('no-deps')`, {cwd: `${path}/packages/workspace-a`}),
          ).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });

          await expect(
            source(`require('one-fixed-dep')`, {cwd: `${path}/packages/workspace-a`}),
          ).resolves.toMatchObject({
            name: `one-fixed-dep`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `it should support profile inheritance (profiles extending other profiles)`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            extends: [`fullstack`],
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            workspaceProfiles: {
              typescript: {
                devDependencies: {
                  [`no-deps`]: `1.0.0`,
                },
              },
              testing: {
                devDependencies: {
                  [`one-fixed-dep`]: `1.0.0`,
                },
              },
              fullstack: {
                extends: [`typescript`, `testing`],
                devDependencies: {
                  [`one-range-dep`]: `1.0.0`,
                },
              },
            },
          });

          await run(`install`);

          // devDependencies from fullstack profile
          await expect(
            source(`require('one-range-dep')`, {cwd: `${path}/packages/workspace-a`}),
          ).resolves.toMatchObject({
            name: `one-range-dep`,
            version: `1.0.0`,
          });

          // devDependencies inherited from typescript profile
          await expect(
            source(`require('no-deps')`, {cwd: `${path}/packages/workspace-a`}),
          ).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });

          // devDependencies inherited from testing profile
          await expect(
            source(`require('one-fixed-dep')`, {cwd: `${path}/packages/workspace-a`}),
          ).resolves.toMatchObject({
            name: `one-fixed-dep`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `it should automatically include the default profile`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            // No explicit extends, but default profile should still apply
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            workspaceProfiles: {
              default: {
                devDependencies: {
                  [`no-deps`]: `1.0.0`,
                },
              },
            },
          });

          await run(`install`);

          // The devDependency from the default profile should be available
          await expect(
            source(`require('no-deps')`, {cwd: `${path}/packages/workspace-a`}),
          ).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `it should include default profile alongside explicit profiles`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            extends: [`typescript`],
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            workspaceProfiles: {
              default: {
                devDependencies: {
                  [`one-fixed-dep`]: `1.0.0`,
                },
              },
              typescript: {
                devDependencies: {
                  [`no-deps`]: `1.0.0`,
                },
              },
            },
          });

          await run(`install`);

          // devDependencies from explicit typescript profile
          await expect(
            source(`require('no-deps')`, {cwd: `${path}/packages/workspace-a`}),
          ).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });

          // devDependencies from implicit default profile
          await expect(
            source(`require('one-fixed-dep')`, {cwd: `${path}/packages/workspace-a`}),
          ).resolves.toMatchObject({
            name: `one-fixed-dep`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `it should let workspace devDependencies take precedence over profiles`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            extends: [`typescript`],
            devDependencies: {
              [`no-deps`]: `2.0.0`, // Override the profile version
            },
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            workspaceProfiles: {
              typescript: {
                devDependencies: {
                  [`no-deps`]: `1.0.0`,
                },
              },
            },
          });

          await run(`install`);

          // Workspace's own devDependency version should take precedence
          await expect(
            source(`require('no-deps')`, {cwd: `${path}/packages/workspace-a`}),
          ).resolves.toMatchObject({
            name: `no-deps`,
            version: `2.0.0`,
          });
        },
      ),
    );

    test(
      `it should not apply profiles to regular dependencies`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            extends: [`typescript`],
            dependencies: {
              [`one-fixed-dep`]: `1.0.0`,
            },
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            workspaceProfiles: {
              typescript: {
                devDependencies: {
                  [`no-deps`]: `1.0.0`,
                },
              },
            },
          });

          await run(`install`);

          // Regular dependency should work
          await expect(
            source(`require('one-fixed-dep')`, {cwd: `${path}/packages/workspace-a`}),
          ).resolves.toMatchObject({
            name: `one-fixed-dep`,
            version: `1.0.0`,
          });

          // devDependency from profile should also work
          await expect(
            source(`require('no-deps')`, {cwd: `${path}/packages/workspace-a`}),
          ).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `it should apply profiles independently to multiple workspaces`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            extends: [`typescript`],
          },
          [`packages/workspace-b`]: {
            name: `workspace-b`,
            version: `1.0.0`,
            extends: [`testing`],
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            workspaceProfiles: {
              typescript: {
                devDependencies: {
                  [`no-deps`]: `1.0.0`,
                },
              },
              testing: {
                devDependencies: {
                  [`one-fixed-dep`]: `1.0.0`,
                },
              },
            },
          });

          await run(`install`);

          // workspace-a should have typescript profile's devDependencies
          await expect(
            source(`require('no-deps')`, {cwd: `${path}/packages/workspace-a`}),
          ).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });

          // workspace-b should have testing profile's devDependencies
          await expect(
            source(`require('one-fixed-dep')`, {cwd: `${path}/packages/workspace-b`}),
          ).resolves.toMatchObject({
            name: `one-fixed-dep`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `it should resolve later profiles over earlier ones when they define the same dependency`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            extends: [`first`, `second`],
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            workspaceProfiles: {
              first: {
                devDependencies: {
                  [`no-deps`]: `1.0.0`,
                },
              },
              second: {
                devDependencies: {
                  [`no-deps`]: `2.0.0`,
                },
              },
            },
          });

          await run(`install`);

          // The second profile's version should take precedence
          await expect(
            source(`require('no-deps')`, {cwd: `${path}/packages/workspace-a`}),
          ).resolves.toMatchObject({
            name: `no-deps`,
            version: `2.0.0`,
          });
        },
      ),
    );

    test(
      `it should work with version ranges in profiles`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            extends: [`typescript`],
          },
        },
        async ({path, run, source}) => {
          await yarn.writeConfiguration(path, {
            workspaceProfiles: {
              typescript: {
                devDependencies: {
                  [`no-deps`]: `^1.0.0`,
                },
              },
            },
          });

          await run(`install`);

          // Should resolve to the highest compatible version
          await expect(
            source(`require('no-deps')`, {cwd: `${path}/packages/workspace-a`}),
          ).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.1.0`,
          });
        },
      ),
    );
  });
});
