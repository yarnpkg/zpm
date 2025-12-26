import {Filename, ppath, PortablePath, xfs} from '@yarnpkg/fslib';
import {yarn}                               from 'pkg-tests-core';

describe(`Features`, () => {
  describe(`Node.js Versioning`, () => {
    test(
      `it should make the managed Node.js available through yarn node`,
      makeTemporaryEnv({
        dependencies: {
          [`@builtin/node`]: `^22.0.0`,
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
          [`@builtin/node`]: `^22.0.0`,
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
          [`@builtin/node`]: `^22.0.0`,
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

    describe(`Monorepo support`, () => {
      test(
        `it should allow declaring @builtin/node in a workspace profile`,
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
                    [`@builtin/node`]: `builtin:^22.0.0`,
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
            [`@builtin/node`]: `^20.0.0`,
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
            [`@builtin/node`]: `^22.0.0`,
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
        `it should by default only fetch the @builtin/node package for the current platform`,
        makeTemporaryEnv({
          dependencies: {
            [`@builtin/node`]: `^22.0.0`,
          },
        }, async ({path, run, source}) => {
          await run(`install`, {
            env: {
              YARN_CPU_OVERRIDE: `x64`,
              YARN_OS_OVERRIDE: `linux`,
            },
          });

          const allCachedFiles = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));
          const nodeFiles = allCachedFiles.sort().filter(file => file.startsWith(`@builtin-node-`));

          expect(nodeFiles).toEqual([
            expect.stringMatching(/@builtin-node-linux-x64-builtin-22\.0\.0-/),
          ]);
        }),
      );

      test(
        `it should fetch @builtin/node packages for multiple platforms when supportedArchitectures is configured`,
        makeTemporaryEnv({
          dependencies: {
            [`@builtin/node`]: `^22.0.0`,
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
          const nodeFiles = allCachedFiles.sort().filter(file => file.startsWith(`@builtin-node-`));

          expect(nodeFiles).toEqual([
            expect.stringMatching(/@builtin-node-darwin-x64-builtin-22\.0\.0-/),
            expect.stringMatching(/@builtin-node-linux-x64-builtin-22\.0\.0-/),
          ]);
        }),
      );

      test(
        `it should produce a stable lockfile regardless of the current platform`,
        makeTemporaryEnv({
          dependencies: {
            [`@builtin/node`]: `^22.0.0`,
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
            [`@builtin/node`]: `^22.0.0`,
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
          const nodeFiles = allCachedFiles.sort().filter(file => file.startsWith(`@builtin-node-`));

          expect(nodeFiles).toEqual([
            expect.stringMatching(/@builtin-node-linux-arm64-builtin-22\.0\.0-/),
            expect.stringMatching(/@builtin-node-linux-x64-builtin-22\.0\.0-/),
          ]);
        }),
      );
    });
  });
});
