import {ppath, xfs, Filename, npath} from '@yarnpkg/fslib';
import {exec, tests}                 from 'pkg-tests-core';

const {setPackageWhitelist} = tests;

describe(`Commands`, () => {
  describe(`dedupe`, () => {
    it(
      `should include a footer`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await setPackageWhitelist(new Map([
          [`no-deps`, new Set([`1.0.0`])],
          [`@types/is-number`, new Set([`1.0.0`])],
        ]), async () => {
          await run(`add`, `two-range-deps`);
        });

        await run(`add`, `no-deps@1.1.0`, `@types/is-number@2.0.0`);

        await expect(run(`dedupe`, `--check`)).rejects.toMatchObject({
          stdout: expect.stringContaining(`2 packages can be deduped using the highest strategy`),
        });
      }),
    );

    it(
      `should build only after deduplicating the dependency graph`,
      makeTemporaryEnv({
        scripts: {
          postinstall: `node -e "require('fs').appendFileSync('built-version', require('one-range-dep').dependencies['no-deps'].version + '\\n')"`,
        },
      }, async ({path, run}) => {
        await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
          await run(`add`, `one-range-dep`, `--mode=update-lockfile`);
        });
        await run(`add`, `no-deps@1.1.0`, `--mode=update-lockfile`);

        await run(`dedupe`);

        expect(await xfs.readFilePromise(ppath.join(path, `built-version`), `utf8`)).toEqual(`1.1.0\n`);
      }),
    );

    describe(`installation modes`, () => {
      for (const enableGlobalCache of [false, true]) {
        for (const args of [[], [`--check`]]) {
          it(
            `should restore a deleted ${enableGlobalCache ? `global` : `local`} cache during dedupe ${args.join(` `)}`,
            makeTemporaryEnv({dependencies: {[`no-deps`]: `1.0.0`}}, {enableGlobalCache}, async ({path, run}) => {
              await run(`dedupe`);
              const cachePath = ppath.join(path, enableGlobalCache ? `.yarn/global/cache` : `.yarn/cache`);
              const previousCache = await xfs.readdirPromise(cachePath);
              await xfs.removePromise(cachePath);

              await run(`dedupe`, ...args);

              expect(await xfs.readdirPromise(cachePath)).toEqual(expect.arrayContaining(previousCache));
              await expect(exec.execFile(process.execPath, [
                `--require`, npath.fromPortablePath(ppath.join(path, Filename.pnpCjs)), `-p`, `require('no-deps').version`,
              ], {cwd: path, env: {...process.env, NODE_OPTIONS: ``}})).resolves.toMatchObject({stdout: `1.0.0\n`});
            }),
          );
        }
      }

      for (const args of [[`--check`], [`--mode=update-lockfile`]]) {
        it(
          `should preserve the linked tree's archives with ${args.join(` `)} after a manifest change`,
          makeTemporaryEnv({dependencies: {[`no-deps`]: `1.0.0`}}, async ({path, run}) => {
            await run(`install`);
            const pnpPath = ppath.join(path, Filename.pnpCjs);
            const previousPnp = await xfs.readFilePromise(pnpPath);
            const statePath = ppath.join(path, `.yarn/ignore/install`);
            const previousState = await xfs.readFilePromise(statePath);
            const cachePath = ppath.join(path, `.yarn/cache`);
            const previousCache = await xfs.readdirPromise(cachePath);

            await xfs.writeJsonPromise(ppath.join(path, Filename.manifest), {
              dependencies: {[`no-deps`]: `1.1.0`},
            });
            await run(`dedupe`, ...args);

            expect(await xfs.readFilePromise(pnpPath)).toEqual(previousPnp);
            expect(await xfs.readFilePromise(statePath)).toEqual(previousState);
            expect(await xfs.readdirPromise(cachePath)).toEqual(expect.arrayContaining(previousCache));
            // Bypass lazy installation so it cannot hide a broken linked tree.
            await expect(exec.execFile(process.execPath, [
              `--require`, npath.fromPortablePath(pnpPath), `-p`, `require('no-deps').version`,
            ], {cwd: path, env: {...process.env, NODE_OPTIONS: ``}})).resolves.toMatchObject({stdout: `1.0.0\n`});

            await run(`install`);
            expect(await xfs.readdirPromise(cachePath)).not.toEqual(expect.arrayContaining(previousCache));
          }),
        );
      }

      it(
        `should leave a conflicted lockfile untouched when checking`,
        makeTemporaryEnv({}, async ({path, run}) => {
          await run(`install`);
          const lockfilePath = ppath.join(path, Filename.lockfile);
          const conflict = `<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> branch\n`;
          await xfs.writeFilePromise(lockfilePath, conflict);
          await expect(run(`dedupe`, `--check`)).rejects.toMatchObject({code: 1});
          expect(await xfs.readFilePromise(lockfilePath, `utf8`)).toEqual(conflict);
        }),
      );

      it(
        `should check the full graph after a production focus`,
        makeTemporaryEnv({}, async ({run}) => {
          await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
            await run(`add`, `-D`, `one-range-dep`);
          });
          await run(`add`, `-D`, `no-deps@1.1.0`);
          await run(`workspaces`, `focus`, `--all`, `--production`);
          await expect(run(`dedupe`, `--check`)).rejects.toMatchObject({code: 1});
        }),
      );

      it(
        `should reuse a complete deduplicated installation only without a mode`,
        makeTemporaryEnv({dependencies: {[`no-deps`]: `1.0.0`}}, async ({path, run}) => {
          await run(`dedupe`);
          const pnpPath = ppath.join(path, Filename.pnpCjs);
          await xfs.writeFilePromise(pnpPath, `corrupted`);

          await run(`dedupe`);
          expect(await xfs.readFilePromise(pnpPath, `utf8`)).toEqual(`corrupted`);

          await run(`dedupe`, `--mode=skip-build`);
          expect(await xfs.readFilePromise(pnpPath, `utf8`)).not.toEqual(`corrupted`);
        }),
      );

      for (const state of [`missing`, `stale`]) {
        for (const command of [[`--mode=update-lockfile`], [`--mode=skip-build`], [`--check`], [`--check`, `--json`]]) {
          it(
            `should respect ${command.join(` `)} with ${state} installation state`,
            makeTemporaryEnv({}, async ({path, run, source}) => {
              // Keep two compatible versions locked without creating install artifacts.
              await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
                await run(`add`, `one-range-dep`, `--mode=update-lockfile`);
              });
              await run(`add`, `no-deps@1.1.0`, `--mode=update-lockfile`);

              const statePath = ppath.join(path, `.yarn/ignore/install`);
              if (state === `stale`) {
                await run(`install`);
              }
              const previousState = await xfs.existsPromise(statePath)
                ? await xfs.readFilePromise(statePath)
                : null;

              // A new scripted dependency makes the previous installation stale.
              const manifestPath = ppath.join(path, Filename.manifest);
              const manifest = await xfs.readJsonPromise(manifestPath);
              manifest.dependencies[`no-deps-scripted`] = `1.0.0`;
              await xfs.writeJsonPromise(manifestPath, manifest);

              const lockfilePath = ppath.join(path, Filename.lockfile);
              const previousLockfile = await xfs.readFilePromise(lockfilePath, `utf8`);
              const pnpPath = ppath.join(path, Filename.pnpCjs);
              const previousPnp = await xfs.existsPromise(pnpPath)
                ? await xfs.readFilePromise(pnpPath)
                : null;

              const buildStatePath = ppath.join(path, `.yarn/ignore/build`);
              const previousBuildState = await xfs.existsPromise(buildStatePath)
                ? await xfs.readFilePromise(buildStatePath)
                : null;

              const checking = command.includes(`--check`);
              if (checking) {
                await expect(run(`dedupe`, ...command, {enableAutoDedupe: true})).rejects.toMatchObject({
                  code: 1,
                  stdout: command.includes(`--json`)
                    ? expect.stringContaining(`"currentResolution":"no-deps@npm:1.0.0"`)
                    : expect.stringContaining(`1 package can be deduped`),
                });
                expect(await xfs.readFilePromise(lockfilePath, `utf8`)).toEqual(previousLockfile);
              } else {
                await run(`dedupe`, ...command);
                const lockfile = await xfs.readJsonPromise(lockfilePath);
                expect(lockfile.entries[`no-deps@npm:1.1.0, no-deps@npm:^1.0.0`].resolution.version).toEqual(`1.1.0`);
              }

              expect(await xfs.existsPromise(buildStatePath) ? await xfs.readFilePromise(buildStatePath) : null).toEqual(previousBuildState);
              if (!command.includes(`--mode=skip-build`)) {
                expect(await xfs.existsPromise(statePath) ? await xfs.readFilePromise(statePath) : null).toEqual(previousState);
                expect(await xfs.existsPromise(pnpPath) ? await xfs.readFilePromise(pnpPath) : null).toEqual(previousPnp);
              } else {
                expect(await xfs.existsPromise(pnpPath)).toBe(true);
              }

              // A later normal install must still link and run pending builds.
              await run(`install`);
              await expect(source(`require('no-deps-scripted/log')`)).resolves.toEqual([`preinstall`, `install`, `postinstall`]);
              await expect(source(`require('one-range-dep').dependencies['no-deps'].version`)).resolves.toEqual(checking ? `1.0.0` : `1.1.0`);
            }),
          );
        }
      }

      it(
        `should check an uninstalled project without creating a lockfile`,
        makeTemporaryEnv({dependencies: {[`no-deps`]: `1.0.0`}}, async ({path, run}) => {
          await run(`dedupe`, `--check`);
          expect(await xfs.existsPromise(ppath.join(path, Filename.lockfile))).toBe(false);
          expect(await xfs.existsPromise(ppath.join(path, Filename.pnpCjs))).toBe(false);
          expect(await xfs.existsPromise(ppath.join(path, `.yarn/ignore/install`))).toBe(false);
        }),
      );
    });

    describe(`strategies`, () => {
      describe(`highest`, () => {
        it(
          `should dedupe dependencies`,
          makeTemporaryEnv({}, async ({path, run, source}) => {
            await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
              await run(`add`, `one-range-dep`);
            });

            await run(`add`, `no-deps@1.1.0`);

            await run(`dedupe`);

            await expect(run(`dedupe`, `--check`)).resolves.toMatchObject({
              code: 0,
            });

            await expect(source(`require('no-deps')`)).resolves.toMatchObject({
              version: `1.1.0`,
            });
            await expect(source(`require('one-range-dep')`)).resolves.toMatchObject({
              dependencies: {
                [`no-deps`]: {
                  version: `1.1.0`,
                },
              },
            });
          }),
        );

        it(
          `should dedupe dependencies to the highest possible version`,
          makeTemporaryEnv({}, async ({path, run, source}) => {
            await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
              await run(`add`, `one-range-dep`, `one-range-dep-too`);
            });

            await run(`add`, `no-deps@1.1.0`);

            await run(`dedupe`);

            await expect(source(`require('no-deps')`)).resolves.toMatchObject({
              version: `1.1.0`,
            });
            await expect(source(`require('one-range-dep')`)).resolves.toMatchObject({
              dependencies: {
                [`no-deps`]: {
                  version: `1.1.0`,
                },
              },
            });
            await expect(source(`require('one-range-dep-too')`)).resolves.toMatchObject({
              dependencies: {
                [`no-deps`]: {
                  version: `1.1.0`,
                },
              },
            });
          }),
        );

        it(
          `should reject malformed lockfiles even with an existing installation state`,
          makeTemporaryEnv({
            dependencies: {[`no-deps`]: `latest`},
          }, async ({path, run, source}) => {
            await run(`install`);
            await run(`add`, `one-range-dep`);

            const lockFilePath = ppath.join(path, Filename.lockfile);
            let lockContent = await xfs.readFilePromise(lockFilePath, `utf8`);

            lockContent = lockContent.replace(`"no-deps@npm:2.0.0"`, `"no-deps@npm:2.0.0::__archiveUrl=https%3A%2F%2Fregistry.com%2Fno-deps-2.0.0.tgz"`);
            await xfs.writeFilePromise(lockFilePath, lockContent);

            // Berry's __archiveUrl syntax is invalid in the native lockfile.
            // A cached install state must not hide this error.
            await expect(run(`dedupe`, `--check`)).rejects.toMatchObject({
              code: 1,
              stdout: expect.stringContaining(`Invalid reference:`),
            });
          }),
        );

        it(
          `should handle aliased packages`,
          makeTemporaryEnv({
            dependencies: {
              [`no-deps`]: `npm:no-deps-bins@^1.0.0`,
              [`one-range-dep`]: `1.0.0`,
            },
          }, async ({path, run, source}) => {
            await run(`install`);

            await run(`dedupe`);

            await expect(run(`dedupe`, `--check`)).resolves.toMatchObject({
              code: 0,
            });

            await expect(source(`require('no-deps')`)).resolves.toMatchObject({
              name: `no-deps-bins`,
              version: `1.0.0`,
            });
          }),
        );
      });
    });

    describe(`patterns`, () => {
      it(
        `should support selective dedupe (ident)`,
        makeTemporaryEnv({}, async ({path, run, source}) => {
          await setPackageWhitelist(new Map([
            [`no-deps`, new Set([`1.0.0`])],
            [`@types/is-number`, new Set([`1.0.0`])],
          ]), async () => {
            await run(`add`, `two-range-deps`);
          });

          await run(`add`, `no-deps@1.1.0`, `@types/is-number@2.0.0`);

          await run(`dedupe`, `no-deps`);

          await expect(source(`require('two-range-deps')`)).resolves.toMatchObject({
            dependencies: {
              [`no-deps`]: {
                version: `1.1.0`,
              },
              [`@types/is-number`]: {
                version: `1.0.0`,
              },
            },
          });
        }),
      );

      it(
        `should support selective dedupe (scoped ident)`,
        makeTemporaryEnv({}, async ({path, run, source}) => {
          await setPackageWhitelist(new Map([
            [`no-deps`, new Set([`1.0.0`])],
            [`@types/is-number`, new Set([`1.0.0`])],
          ]), async () => {
            await run(`add`, `two-range-deps`);
          });

          await run(`add`, `no-deps@1.1.0`, `@types/is-number@2.0.0`);

          await run(`dedupe`, `@types/is-number`);

          await expect(source(`require('two-range-deps')`)).resolves.toMatchObject({
            dependencies: {
              [`no-deps`]: {
                version: `1.0.0`,
              },
              [`@types/is-number`]: {
                version: `2.0.0`,
              },
            },
          });
        }),
      );

      it(
        `should support selective dedupe (ident glob)`,
        makeTemporaryEnv({}, async ({path, run, source}) => {
          await setPackageWhitelist(new Map([
            [`no-deps`, new Set([`1.0.0`])],
            [`@types/is-number`, new Set([`1.0.0`])],
          ]), async () => {
            await run(`add`, `two-range-deps`);
          });

          await run(`add`, `no-deps@1.1.0`, `@types/is-number@2.0.0`);

          await run(`dedupe`, `no-*`);

          await expect(source(`require('two-range-deps')`)).resolves.toMatchObject({
            dependencies: {
              [`no-deps`]: {
                version: `1.1.0`,
              },
              [`@types/is-number`]: {
                version: `1.0.0`,
              },
            },
          });
        }),
      );

      it(
        `should support selective dedupe (scoped ident glob)`,
        makeTemporaryEnv({}, async ({path, run, source}) => {
          await setPackageWhitelist(new Map([
            [`no-deps`, new Set([`1.0.0`])],
            [`@types/is-number`, new Set([`1.0.0`])],
          ]), async () => {
            await run(`add`, `two-range-deps`);
          });

          await run(`add`, `no-deps@1.1.0`, `@types/is-number@2.0.0`);

          await run(`dedupe`, `@types/*`);

          await expect(source(`require('two-range-deps')`)).resolves.toMatchObject({
            dependencies: {
              [`no-deps`]: {
                version: `1.0.0`,
              },
              [`@types/is-number`]: {
                version: `2.0.0`,
              },
            },
          });
        }),
      );
    });

    describe(`flags`, () => {
      describe(`-c,--check`, () => {
        it(
          `should reject with error code 1 when there are duplicates`,
          makeTemporaryEnv({}, async ({path, run, source}) => {
            await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
              await run(`add`, `one-range-dep`);
            });

            await run(`add`, `no-deps@1.1.0`);

            await expect(run(`dedupe`, `--check`)).rejects.toMatchObject({
              code: 1,
            });
          }),
        );

        it(
          `should resolve with error code 0 when there are no duplicates`,
          makeTemporaryEnv({}, async ({path, run, source}) => {
            await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
              await run(`add`, `one-range-dep`);
            });

            await run(`add`, `no-deps@2.0.0`);

            await expect(run(`dedupe`, `--check`)).resolves.toMatchObject({
              code: 0,
            });
          }),
        );
      });

      test(
        `--json`,
        makeTemporaryEnv({}, async ({path, run, source}) => {
          await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
            await run(`add`, `one-range-dep`);
          });

          await run(`add`, `no-deps@1.1.0`);

          // We also use the check flag so that the stdout doesn't include the install report
          await run(`dedupe`, `--json`, `--check`).catch(({stdout}) => {
            expect(JSON.parse(stdout.trim())).toMatchObject({
              descriptor: `no-deps@npm:^1.0.0`,
              currentResolution: `no-deps@npm:1.0.0`,
              updatedResolution: `no-deps@npm:1.1.0`,
            });
          });

          expect.assertions(1);
        }),
      );

      test(
        `-s,--strategy`,
        makeTemporaryEnv({}, async ({path, run, source}) => {
          await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
            await run(`add`, `one-range-dep`);
          });

          await run(`add`, `no-deps@1.1.0`);

          await expect(run(`dedupe`, `--check`, `--strategy`, `highest`)).rejects.toMatchObject({
            code: 1,
          });
        }),
      );
    });
  });
});
