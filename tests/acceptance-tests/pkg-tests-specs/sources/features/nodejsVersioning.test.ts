import {Filename, ppath, PortablePath, xfs} from '@yarnpkg/fslib';
import {tests, yarn}                        from 'pkg-tests-core';

const {startPackageServer, validNodeDistAuthHeader} = tests;

describe(`Features`, () => {
  describe(`Node.js Versioning`, () => {
    test(
      `it should make the managed Node.js available through yarn node`,
      makeTemporaryEnv({
        dependencies: {
          [`@yarnpkg/node`]: `builtin:^22.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`, {
          env: {
            YARN_CPU_OVERRIDE: `x64`,
            YARN_OS_OVERRIDE: `linux`,
          },
        });

        const {stdout} = await run(`node`, `--version`);
        expect(stdout.trim()).toMatch(/^node-v22.0.0-linux-x64$/);
      }),
    );

    test(
      `it should make the managed Node.js available through yarn exec`,
      makeTemporaryEnv({
        dependencies: {
          [`@yarnpkg/node`]: `builtin:^22.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`, {
          env: {
            YARN_CPU_OVERRIDE: `x64`,
            YARN_OS_OVERRIDE: `linux`,
          },
        });

        const {stdout} = await run(`exec`, `node`, `--version`);
        expect(stdout.trim()).toMatch(/^node-v22.0.0-linux-x64$/);
      }),
    );

    test(
      `it should run scripts with the managed Node.js version`,
      makeTemporaryEnv({
        dependencies: {
          [`@yarnpkg/node`]: `builtin:^22.0.0`,
        },
        scripts: {
          [`check-version`]: `node --version`,
        },
      }, async ({path, run, source}) => {
        await run(`install`, {
          env: {
            YARN_CPU_OVERRIDE: `x64`,
            YARN_OS_OVERRIDE: `linux`,
          },
        });

        const {stdout} = await run(`check-version`);
        expect(stdout.trim()).toMatch(/^node-v22.0.0-linux-x64$/);
      }),
    );

    describe(`Distribution authentication`, () => {
      test(
        `it should send the configured authorization header`,
        makeTemporaryEnv({
          dependencies: {
            [`@yarnpkg/node`]: `builtin:^22.0.0`,
          },
        }, async ({run}) => {
          await run(`install`, {
            nodeDistUrl: `${await startPackageServer()}/node-private/dist`,
            nodeDistAuthHeader: validNodeDistAuthHeader,
            env: {
              YARN_CPU_OVERRIDE: `x64`,
              YARN_OS_OVERRIDE: `linux`,
            },
          });
        }),
      );

      test(
        `it should fail with a descriptive error when the authorization header is invalid`,
        makeTemporaryEnv({
          dependencies: {
            [`@yarnpkg/node`]: `builtin:^22.0.0`,
          },
        }, async ({run}) => {
          await expect(run(`install`, {
            nodeDistUrl: `${await startPackageServer()}/node-private/dist`,
            nodeDistAuthHeader: `Bearer invalid-node-dist-token`,
            env: {
              YARN_CPU_OVERRIDE: `x64`,
              YARN_OS_OVERRIDE: `linux`,
            },
          })).rejects.toThrow(/Network error: HTTP status client error \(401 Unauthorized\) for url .*\/node-private\/dist\/index\.json/);
        }),
      );
    });

    describe(`Monorepo support`, () => {
      test(
        `it should allow declaring @yarnpkg/node in a workspace profile`,
        makeTemporaryMonorepoEnv(
          {
            workspaces: [`packages/*`],
          },
          {
            [`packages/workspace-a`]: {
              name: `workspace-a`,
              version: `1.0.0`,
            },
          },
          async ({path, run, source}) => {
            await yarn.writeConfiguration(path, {
              workspaceProfiles: {
                default: {
                  devDependencies: {
                    [`@yarnpkg/node`]: `builtin:^22.0.0`,
                  },
                },
              },
            });

            await run(`install`, {
              env: {
                YARN_CPU_OVERRIDE: `x64`,
                YARN_OS_OVERRIDE: `linux`,
              },
            });

            // Should be able to use the managed Node.js from the workspace
            const {stdout} = await run(`node`, `--version`, {cwd: `${path}/packages/workspace-a` as PortablePath});
            expect(stdout.trim()).toMatch(/^node-v22.0.0-linux-x64$/);
          },
        ),
      );
    });

    describe(`Different versions`, () => {
      test(
        `it should support Node.js 20.x`,
        makeTemporaryEnv({
          dependencies: {
            [`@yarnpkg/node`]: `builtin:^20.0.0`,
          },
        }, async ({path, run, source}) => {
          await run(`install`, {
            env: {
              YARN_CPU_OVERRIDE: `x64`,
              YARN_OS_OVERRIDE: `linux`,
            },
          });

          const {stdout} = await run(`node`, `--version`);
          expect(stdout.trim()).toMatch(/^node-v20.0.0-linux-x64$/);
        }),
      );

      test(
        `it should support Node.js 22.x`,
        makeTemporaryEnv({
          dependencies: {
            [`@yarnpkg/node`]: `builtin:^22.0.0`,
          },
        }, async ({path, run, source}) => {
          await run(`install`, {
            env: {
              YARN_CPU_OVERRIDE: `x64`,
              YARN_OS_OVERRIDE: `linux`,
            },
          });

          const {stdout} = await run(`node`, `--version`);
          expect(stdout.trim()).toMatch(/^node-v22.0.0-linux-x64$/);
        }),
      );
    });

    describe(`Platform support`, () => {
      test(
        `it should by default only fetch the @yarnpkg/node package for the current platform`,
        makeTemporaryEnv({
          dependencies: {
            [`@yarnpkg/node`]: `builtin:^22.0.0`,
          },
        }, async ({path, run, source}) => {
          await run(`install`, {
            env: {
              YARN_CPU_OVERRIDE: `x64`,
              YARN_OS_OVERRIDE: `linux`,
            },
          });

          const allCachedFiles = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));
          const nodeFiles = allCachedFiles.sort().filter(file => file.startsWith(`@yarnpkg-node-`));

          expect(nodeFiles).toEqual([
            expect.stringMatching(/@yarnpkg-node-linux-x64-builtin-22\.0\.0-/),
          ]);
        }),
      );

      test(
        `it should fetch @yarnpkg/node packages for multiple platforms when supportedArchitectures is configured`,
        makeTemporaryEnv({
          dependencies: {
            [`@yarnpkg/node`]: `builtin:^22.0.0`,
          },
        }, async ({path, run, source}) => {
          await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
            supportedArchitectures: {
              os: [`linux`, `darwin`],
              cpu: [`x64`],
            },
          });

          await run(`install`, {
            env: {
              YARN_CPU_OVERRIDE: `x64`,
              YARN_OS_OVERRIDE: `linux`,
            },
          });

          const allCachedFiles = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));
          const nodeFiles = allCachedFiles.sort().filter(file => file.startsWith(`@yarnpkg-node-`));

          expect(nodeFiles).toEqual([
            expect.stringMatching(/@yarnpkg-node-darwin-x64-builtin-22\.0\.0-/),
            expect.stringMatching(/@yarnpkg-node-linux-x64-builtin-22\.0\.0-/),
          ]);
        }),
      );

      test(
        `it should produce a stable lockfile regardless of the current platform`,
        makeTemporaryEnv({
          dependencies: {
            [`@yarnpkg/node`]: `builtin:^22.0.0`,
          },
        }, async ({path, run, source}) => {
          await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
            supportedArchitectures: {
              os: [`linux`, `darwin`],
              cpu: [`x64`],
            },
          });

          await run(`install`, {
            env: {
              YARN_CPU_OVERRIDE: `x64`,
              YARN_OS_OVERRIDE: `linux`,
            },
          });

          const lockfileLinux = await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`);

          await run(`install`, {
            env: {
              YARN_CPU_OVERRIDE: `x64`,
              YARN_OS_OVERRIDE: `darwin`,
            },
          });

          const lockfileDarwin = await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`);

          expect(lockfileDarwin).toEqual(lockfileLinux);
        }),
      );

      test(
        `it should resolve platform-specific packages for arm64 and x64 when both are configured`,
        makeTemporaryEnv({
          dependencies: {
            [`@yarnpkg/node`]: `builtin:^22.0.0`,
          },
        }, async ({path, run, source}) => {
          await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
            supportedArchitectures: {
              os: [`linux`],
              cpu: [`x64`, `arm64`],
            },
          });

          await run(`install`, {
            env: {
              YARN_CPU_OVERRIDE: `x64`,
              YARN_OS_OVERRIDE: `linux`,
            },
          });

          const allCachedFiles = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));
          const nodeFiles = allCachedFiles.sort().filter(file => file.startsWith(`@yarnpkg-node-`));

          expect(nodeFiles).toEqual([
            expect.stringMatching(/@yarnpkg-node-linux-arm64-builtin-22\.0\.0-/),
            expect.stringMatching(/@yarnpkg-node-linux-x64-builtin-22\.0\.0-/),
          ]);
        }),
      );
    });
  });
});
