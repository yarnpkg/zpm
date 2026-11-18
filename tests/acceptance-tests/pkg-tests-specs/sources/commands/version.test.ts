import {xfs, ppath, PortablePath, Filename} from '@yarnpkg/fslib';
import {exec}                               from 'pkg-tests-core';

describe(`Commands`, () => {
  describe(`version`, () => {
    describe(`immediate mode`, () => {
      test(
        `it should apply incremental strategy to a workspace`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await run(`version`, `patch`);
          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.2.4`,
          });

          await run(`version`, `minor`);
          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.3.0`,
          });

          await run(`version`, `major`);
          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `2.0.0`,
          });
        }),
      );

      test(
        `it should apply semver strategy to a workspace`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await expect(run(`version`, `3.4.5`)).resolves.toMatchObject({
            code: 0,
          });

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `3.4.5`,
          });
        }),
      );

      test(
        `it should not bump a workspace when applying the "decline" strategy`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await run(`version`, `decline`);
          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.2.3`,
          });
        }),
      );

      const BUMPS = {
        stable: {from: `1.2.3`, prepatch: `1.2.4-0`, preminor: `1.3.0-0`, premajor: `2.0.0-0`},
        prepatch: {from: `1.2.3-9`, prepatch: `1.2.3-10`, preminor: `1.3.0-0`, premajor: `2.0.0-0`},
        preminor: {from: `1.2.0-9`, prepatch: `1.2.0-10`, preminor: `1.2.0-10`, premajor: `2.0.0-0`},
        premajor: {from: `1.0.0-9`, prepatch: `1.0.0-10`, preminor: `1.0.0-10`, premajor: `1.0.0-10`},
      };

      for (const strategy of [`prepatch`, `preminor`, `premajor`] as const) {
        for (const [base, versions] of Object.entries(BUMPS)) {
          const isPrereleaseBump = versions[strategy].endsWith(`-10`);
          test(
            isPrereleaseBump
              ? `it should bump a ${base} version to the next prerelease when applying the "${strategy}" strategy`
              : `it should bump a ${base} version to a prerelease of the next ${strategy.slice(3)} version when applying the "${strategy}" strategy`,
            makeTemporaryEnv({
              version: versions.from,
            }, async ({path, run, source}) => {
              await run(`version`, strategy);

              await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
                version: versions[strategy],
              });
            }),
          );
        }
      }

      test(
        `it should bump a stable version to a prerelease of the next patch version when applying the "prerelease" strategy`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await run(`version`, `prerelease`);

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.2.4-0`,
          });
        }),
      );

      test(
        `it should bump a prerelease version to the next prerelease when applying the "prerelease" strategy`,
        makeTemporaryEnv({
          version: `1.2.3-9`,
        }, async ({path, run, source}) => {
          await run(`version`, `prerelease`);

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.2.3-10`,
          });
        }),
      );

      test(
        `it shouldn't work if the strategy isn't semver and there is no prior version`,
        makeTemporaryEnv({}, async ({path, run, source}) => {
          await expect(run(`version`, `patch`)).rejects.toThrow(`Can't bump the version if there wasn't a version to begin with - set an initial version then run the command again.`);
        }),
      );

      test(
        `it should throw when applying an invalid strategy`,
        makeTemporaryEnv({
          version: `1.0.0`,
        }, async ({path, run, source}) => {
          await expect(run(`version`, `invalid`)).rejects.toThrow(`Invalid strategy: "invalid"`);
          // Also checks that errors for near-semver suggests a "cleaned" version
          await expect(run(`version`, `1.2.3-01`)).rejects.toThrow(`1.2.3-1`);
        }),
      );

      test(
        `it should throw when trying to bump to a lower version than the current one`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await expect(run(`version`, `1.0.0`)).rejects.toThrow(/lower/);
        }),
      );

      test(
        `it should bump to a lower version than the current one if --force is set`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await expect(run(`version`, `1.0.0`, `--force`)).resolves.toMatchObject({
            code: 0,
          });

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.0.0`,
          });
        }),
      );

      test(
        `it should apply --prerelease on a regular patch bump`,
        makeTemporaryEnv({
          version: `1.0.32`,
        }, async ({path, run, source}) => {
          await run(`version`, `patch`, `--prerelease=alpha.0`);

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.0.33-alpha.0`,
          });
        }),
      );

      test(
        `it should bump the last numeric prerelease identifier if --prerelease is not set when applying the "prerelease" strategy`,
        makeTemporaryEnv({
          version: `1.2.3-a.1.b.2.c`,
        }, async ({path, run, source}) => {
          await run(`version`, `prerelease`);

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.2.3-a.1.b.3.c`,
          });
        }),
      );

      test(
        `it should bump the prerelease identifiers specified by the --prerelease pattern when applying the "prerelease" strategy`,
        makeTemporaryEnv({
          version: `1.2.3-a.1.b.2.c`,
        }, async ({path, run, source}) => {
          await run(`version`, `prerelease`, `--prerelease=a.%n.b.%n.c`);

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.2.3-a.2.b.3.c`,
          });
        }),
      );

      test(
        `it should restart the prerelease sequence if the --prerelease pattern does not match when applying the "prerelease" strategy`,
        makeTemporaryEnv({
          version: `1.2.3-alpha.999`,
        }, async ({path, run, source}) => {
          await run(`version`, `prerelease`, `--prerelease=beta.%n`);

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.2.3-beta.1`,
          });
        }),
      );

      test(
        `it should restart the prerelease sequence if the --prerelease template is a partial match when applying the "prerelease" strategy`,
        makeTemporaryEnv({
          version: `1.2.3-a.1.b.2.c`,
        }, async ({path, run, source}) => {
          await run(`version`, `prerelease`, `--prerelease=a.%n.b.c.%n`);

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.2.3-a.1.b.c.1`,
          });
        }),
      );

      test(
        `it should immediately bump the version when using --immediate, even if preferDeferredVersions is set`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, {
          preferDeferredVersions: true,
        }, async ({path, run, source}) => {
          await run(`version`, `patch`, `--immediate`);

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.2.4`,
          });
        }),
      );

      test(
        `it should bump all workspaces when using --all`,
        makeTemporaryEnv(
          {
            version: `1.2.3`,
            workspaces: [
              `packages/*`,
            ],
          },
          async ({path, run, source}) => {
            const pkgA = ppath.join(path, `packages/pkg-a`);
            await xfs.mkdirpPromise(pkgA);
            await xfs.writeJsonPromise(ppath.join(pkgA, Filename.manifest), {
              name: `pkg-a`,
              version: `4.5.6`,
            });

            const pkgB = ppath.join(path, `packages/pkg-b`);
            await xfs.mkdirpPromise(pkgB);
            await xfs.writeJsonPromise(ppath.join(pkgB, Filename.manifest), {
              name: `pkg-b`,
              version: `7.8.9`,
            });

            await run(`install`);
            await run(`version`, `patch`, `--all`);

            await expect(xfs.readJsonPromise(ppath.join(pkgA, Filename.manifest))).resolves.toMatchObject({
              version: `4.5.7`,
            });

            await expect(xfs.readJsonPromise(ppath.join(pkgB, Filename.manifest))).resolves.toMatchObject({
              version: `7.8.10`,
            });
          }),
      );

      // Not ported: "it should recursively bump dependent workspaces when using --recursive"
      // (zpm doesn't implement --recursive, see breaking-changes.md)

      // Not ported: "it should update workspace references when applying version changes" and
      // "it should remove range from workspace references when using --exact" (zpm doesn't
      // rewrite the dependents' ranges, see breaking-changes.md)

      test(
        `it should correctly report a dependent workspace when unable to upgrade its version.`,
        makeTemporaryEnv(
          {
            private: true,
            workspaces: [
              `packages/*`,
            ],
          },
          async ({path, run, source}) => {
            // Create the primary package.
            const pkgPrimary = ppath.join(path, `packages/pkg-primary`);
            await xfs.mkdirpPromise(pkgPrimary);
            await xfs.writeJsonPromise(ppath.join(pkgPrimary, Filename.manifest), {
              name: `pkg-primary`,
              version: `1.0.0`,
            });

            // Create the dependant package.
            const pkgDependant = ppath.join(path, `packages/pkg-dependant`);
            await xfs.mkdirpPromise(pkgDependant);
            await xfs.writeJsonPromise(ppath.join(pkgDependant, Filename.manifest), {
              name: `pkg-dependant`,
              version: `1.0.0`,
              dependencies: {
                [`pkg-primary`]: `workspace:*`,
              },
            });

            await run(`install`);

            await expect(run(`workspace`, `pkg-primary`, `version`, `patch`)).resolves.toMatchObject({
              code: 0,
              stdout: expect.stringContaining(`Couldn't auto-upgrade range workspace:* (in pkg-dependant@workspace:packages/pkg-dependant)`),
            });
          }),
      );

      test(
        `it should also report workspace:^/~ ranges and explicit workspace semver ranges`,
        makeTemporaryEnv(
          {
            private: true,
            workspaces: [
              `packages/*`,
            ],
          },
          async ({path, run, source}) => {
            const pkgPrimary = ppath.join(path, `packages/pkg-primary`);
            await xfs.mkdirpPromise(pkgPrimary);
            await xfs.writeJsonPromise(ppath.join(pkgPrimary, Filename.manifest), {
              name: `pkg-primary`,
              version: `1.0.0`,
            });

            const caretWs = ppath.join(path, `packages/pkg-caret`);
            await xfs.mkdirpPromise(caretWs);
            await xfs.writeJsonPromise(ppath.join(caretWs, Filename.manifest), {
              name: `pkg-caret`,
              version: `1.0.0`,
              dependencies: {[`pkg-primary`]: `workspace:^`},
            });

            const semverWs = ppath.join(path, `packages/pkg-semver`);
            await xfs.mkdirpPromise(semverWs);
            await xfs.writeJsonPromise(ppath.join(semverWs, Filename.manifest), {
              name: `pkg-semver`,
              version: `1.0.0`,
              dependencies: {[`pkg-primary`]: `workspace:^1.0.0`},
            });

            const peerWs = ppath.join(path, `packages/pkg-peer`);
            await xfs.mkdirpPromise(peerWs);
            await xfs.writeJsonPromise(ppath.join(peerWs, Filename.manifest), {
              name: `pkg-peer`,
              version: `1.0.0`,
              peerDependencies: {[`pkg-primary`]: `workspace:~1.0.0`},
            });

            await run(`install`);

            const {stdout} = await run(`workspace`, `pkg-primary`, `version`, `major`);
            expect(stdout).toContain(`Couldn't auto-upgrade range workspace:^ (in pkg-caret@workspace:packages/pkg-caret)`);
            expect(stdout).toContain(`Couldn't auto-upgrade range workspace:^1.0.0 (in pkg-semver@workspace:packages/pkg-semver)`);
            expect(stdout).toContain(`Couldn't auto-upgrade range workspace:~1.0.0 (in pkg-peer@workspace:packages/pkg-peer)`);
          }),
      );

      test(
        `it should remove the stableVersion field when bumping to a stable version`,
        makeTemporaryEnv({
          version: `1.2.3-rc.1`,
          stableVersion: `1.2.2`,
        }, async ({path, run, source}) => {
          await run(`version`, `patch`);

          const manifest = await xfs.readJsonPromise(`${path}/package.json` as PortablePath);
          expect(manifest).toMatchObject({
            version: `1.2.3`,
          });
          expect(manifest).not.toHaveProperty(`stableVersion`);
        }),
      );

      test(
        `it should ignore and preserve the stableVersion field when bumping to a prerelease`,
        makeTemporaryEnv({
          version: `1.2.3-rc.1`,
          stableVersion: `1.2.2`,
        }, async ({path, run, source}) => {
          await run(`version`, `prerelease`);

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.2.3-rc.2`,
            stableVersion: `1.2.2`,
          });
        }),
      );

      test(
        `it should not actually bump when --dry-run is set`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await expect(run(`version`, `patch`, `--dry-run`)).resolves.toMatchObject({
            code: 0,
            stdout: expect.stringContaining(`1.2.4`),
          });

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.2.3`,
          });
        }),
      );

      test(
        `it should work outside of a git repository`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await expect(xfs.existsPromise(ppath.join(path, `.git`))).resolves.toBe(false);

          await run(`version`, `patch`, `--immediate`);

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.2.4`,
          });
        }),
      );

      test(
        `it should work in a newly-initialized git repository`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await exec.execGitInit({cwd: path});

          await run(`version`, `patch`, `--immediate`);

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.2.4`,
          });
        }),
      );
    });

    describe(`deferred mode`, () => {
      test(
        `it should apply a deferred incremental strategy to a workspace`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await run(`version`, `patch`, `--deferred`);
          await run(`version`, `apply`);

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.2.4`,
          });
        }),
      );

      test(
        `it should apply a deferred semver strategy to a workspace`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await run(`version`, `4.5.6`, `--deferred`);
          await run(`version`, `apply`);

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `4.5.6`,
          });
        }),
      );

      test(
        `it should apply the highest deferred bump when multiple deferred bumps are recorded`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await run(`version`, `patch`, `--deferred`);
          await run(`version`, `major`, `--deferred`);
          await run(`version`, `minor`, `--deferred`);
          await run(`version`, `apply`);

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `2.0.0`,
          });

          await run(`version`, `minor`, `--deferred`);
          await run(`version`, `2.1.3-rc.1`, `--deferred`);
          await run(`version`, `apply`);

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `2.1.3-rc.1`,
          });
        }),
      );

      test(
        `it should throw when deferring an invalid strategy`,
        makeTemporaryEnv({
          version: `1.0.0`,
        }, async ({path, run, source}) => {
          await expect(run(`version`, `invalid`, `--deferred`)).rejects.toThrow(`Invalid strategy: "invalid"`);
          // Also checks that errors for near-semver suggests a "cleaned" version
          await expect(run(`version`, `1.2.3-01`)).rejects.toThrow(`1.2.3-1`);
        }),
      );

      test(
        `it should throw when trying to defer a lower version than the current one`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await expect(run(`version`, `1.0.0`, `--deferred`)).rejects.toThrow(/lower/);
        }),
      );

      test(
        `it should defer a lower version than the current one if --force is set`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await expect(run(`version`, `1.0.0`, `--deferred`, `--force`)).resolves.toMatchObject({
            code: 0,
          });
        }),
      );

      test(
        `it should throw when trying to apply a lower version than the current one`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await run(`version`, `1.0.0`, `--deferred`, `--force`);
          await expect(run(`version`, `apply`)).rejects.toThrow(/lower/);
        }),
      );

      test(
        `it should apply a lower version than the current one if --force is set`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await run(`version`, `1.0.0`, `--deferred`, `--force`);

          await expect(run(`version`, `apply`, `--force`)).resolves.toMatchObject({
            code: 0,
          });

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.0.0`,
          });
        }),
      );

      test(
        `it shouldn't immediately bump the version when using --deferred`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await run(`version`, `patch`, `--deferred`);

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.2.3`,
          });
        }),
      );

      test(
        `it shouldn't immediately bump the version when preferDeferredVersions is set`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, {
          preferDeferredVersions: true,
        }, async ({path, run, source}) => {
          await run(`version`, `patch`);

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.2.3`,
          });
        }),
      );

      test(
        `it should apply a deferred prerelease strategy to a workspace`,
        makeTemporaryEnv({
          version: `1.2.0-3`,
        }, async ({path, run, source}) => {
          await run(`version`, `prepatch`, `--deferred`);
          await run(`version`, `apply`);

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.2.0-4`,
          });
        }),
      );

      test(
        `it should successfully record "decline" on top of the stored version`,
        makeTemporaryEnv({
          version: `1.0.0`,
        }, async ({path, run, source}) => {
          await run(`version`, `major`, `--deferred`);

          await expect(run(`version`, `decline`, `--deferred`)).resolves.toMatchObject({
            code: 0,
          });

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.0.0`,
          });
        }),
      );

      test(
        `it shouldn't record the version bump when --dry-run is set`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await run(`version`, `patch`, `--deferred`, `--dry-run`);
          await run(`version`, `apply`);

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.2.3`,
          });
        }),
      );
    });

    describe(`mixed mode`, () => {
      test(
        `it shouldn't work if the immediate bump would be lower than the planned version (incremental strategy)`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await run(`version`, `major`, `--deferred`);
          await expect(run(`version`, `patch`)).rejects.toThrow(`Can't bump the version to one that would be lower than the current deferred one (2.0.0)`);
        }),
      );

      test(
        `it shouldn't work if the immediate bump would be lower than the planned version (semver strategy)`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await run(`version`, `major`, `--deferred`);
          await expect(run(`version`, `1.4.5`)).rejects.toThrow(`Can't bump the version to one that would be lower than the current deferred one (2.0.0)`);
        }),
      );

      test(
        `it should work if the immediate bump is equal to the planned version (incremental strategy)`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await run(`version`, `1.3.0`, `--deferred`);
          await run(`version`, `minor`);

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.3.0`,
          });
        }),
      );

      test(
        `it should work if the immediate bump is equal to the planned version (semver strategy)`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await run(`version`, `minor`, `--deferred`);
          await run(`version`, `1.3.0`);

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.3.0`,
          });
        }),
      );

      test(
        `it should work if the immediate bump is greater than the planned version (incremental strategy)`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await run(`version`, `minor`, `--deferred`);
          await run(`version`, `major`);

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `2.0.0`,
          });
        }),
      );

      test(
        `it should work if the immediate bump is greater than the planned version (semver strategy)`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await run(`version`, `minor`, `--deferred`);
          await run(`version`, `3.4.5`);

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `3.4.5`,
          });
        }),
      );

      test(
        `it should successfully apply "decline" on top of the stored version`,
        makeTemporaryEnv({
          version: `1.0.0`,
        }, async ({path, run, source}) => {
          await run(`version`, `major`, `--deferred`);

          await expect(run(`version`, `decline`)).resolves.toMatchObject({
            code: 0,
          });

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `2.0.0`,
          });
        }),
      );

      test(
        `it should successfully apply a version bump that can't be described by a strategy on top of the stored version`,
        makeTemporaryEnv({
          version: `1.0.0`,
        }, async ({path, run, source}) => {
          await run(`version`, `major`, `--deferred`);

          await expect(run(`version`, `3.4.5`)).resolves.toMatchObject({
            code: 0,
          });

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `3.4.5`,
          });
        }),
      );

      test(
        `it should throw when applying an invalid strategy on top of the stored version`,
        makeTemporaryEnv({
          version: `1.0.0`,
        }, async ({path, run, source}) => {
          await run(`version`, `major`, `--deferred`);

          await expect(run(`version`, `invalid`)).rejects.toThrow(`Invalid strategy: "invalid"`);
          // Also checks that errors for near-semver suggests a "cleaned" version
          await expect(run(`version`, `1.2.3-01`)).rejects.toThrow(`1.2.3-1`);
        }),
      );

      test(
        `it should work if the immediate bump is lower than the planned version when --force is set (incremental strategy)`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await run(`version`, `major`, `--deferred`);
          await expect(run(`version`, `patch`, `--force`)).resolves.toMatchObject({
            code: 0,
          });

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.2.4`,
          });
        }),
      );

      test(
        `it should work if the immediate bump is lower than the planned version when --force is set (semver strategy)`,
        makeTemporaryEnv({
          version: `1.2.3`,
        }, async ({path, run, source}) => {
          await run(`version`, `major`, `--deferred`);
          await expect(run(`version`, `1.4.5`, `--force`)).resolves.toMatchObject({
            code: 0,
          });

          await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
            version: `1.4.5`,
          });
        }),
      );
    });
  });
});
