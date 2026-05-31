import {xfs, ppath, PortablePath, Filename} from '@yarnpkg/fslib';

describe(`Commands`, () => {
  describe(`version check`, () => {
    test(
      `it shouldn't work if the strategy isn't semver and there is no prior version`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await expect(run(`version`, `patch`)).rejects.toThrow(`Can't bump the version if there wasn't a version to begin with - use 0.0.0 as initial version then run the command again.`);
      }),
    );

    test(
      `it shouldn't work if the immediate bump would be lower than the planned version (semver strategy)`,
      makeTemporaryEnv({
        version: `1.0.0`,
      }, async ({path, run, source}) => {
        await run(`version`, `1.1.0`, `--deferred`);
        await expect(run(`version`, `1.0.1`)).rejects.toThrow(`Can't bump the version to one that would be lower than the current deferred one (1.1.0)`);
      }),
    );

    test(
      `it shouldn't work if the immediate bump would be lower than the planned version (incremental strategy)`,
      makeTemporaryEnv({
        version: `1.0.0`,
      }, async ({path, run, source}) => {
        await run(`version`, `1.1.0`, `--deferred`);
        await expect(run(`version`, `patch`)).rejects.toThrow(`Can't bump the version to one that would be lower than the current deferred one (1.1.0)`);
      }),
    );

    test(
      `it should work if the immediate bump is greater than the planned version (semver strategy)`,
      makeTemporaryEnv({
        version: `1.0.0`,
      }, async ({path, run, source}) => {
        await run(`version`, `1.1.0`, `--deferred`);
        await run(`version`, `2.0.0`);

        await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
          version: `2.0.0`,
        });
      }),
    );

    test(
      `it should work if the immediate bump is greater than the planned version (incremental strategy)`,
      makeTemporaryEnv({
        version: `1.0.0`,
      }, async ({path, run, source}) => {
        await run(`version`, `1.1.0`, `--deferred`);
        await run(`version`, `major`);

        await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
          version: `2.0.0`,
        });
      }),
    );

    test(
      `it should work if the immediate bump is equal to the planned version (semver strategy)`,
      makeTemporaryEnv({
        version: `1.0.0`,
      }, async ({path, run, source}) => {
        await run(`version`, `1.1.0`, `--deferred`);
        await run(`version`, `1.1.0`);

        await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
          version: `1.1.0`,
        });
      }),
    );

    test(
      `it should work if the immediate bump is equal to the planned version (incremental strategy)`,
      makeTemporaryEnv({
        version: `1.0.0`,
      }, async ({path, run, source}) => {
        await run(`version`, `1.1.0`, `--deferred`);
        await run(`version`, `minor`);

        await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
          version: `1.1.0`,
        });
      }),
    );

    test(
      `it should increase the version number for a workspace`,
      makeTemporaryEnv({
        version: `0.0.0`,
      }, async ({path, run, source}) => {
        await run(`version`, `patch`);

        await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
          version: `0.0.1`,
        });
      }),
    );

    test(
      `it should bump then append a prerelease version number to a release version`,
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
      `it should bump the prerelease version number on a prerelease version`,
      makeTemporaryEnv({
        version: `11.22.33-9`,
      }, async ({path, run, source}) => {
        await run(`version`, `prerelease`);

        await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
          version: `11.22.33-10`,
        });
      }),
    );

    test(
      `it shouldn't immediately increase the version number for a workspace when using --deferred`,
      makeTemporaryEnv({
        version: `0.0.0`,
      }, async ({path, run, source}) => {
        await run(`version`, `patch`, `--deferred`);

        await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
          version: `0.0.0`,
        });

        await run(`version`, `apply`);

        await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
          version: `0.0.1`,
        });
      }),
    );

    test(
      `it shouldn't immediately increase the version number for a workspace when using preferDeferredVersions`,
      makeTemporaryEnv({
        version: `0.0.0`,
      }, {
        preferDeferredVersions: true,
      }, async ({path, run, source}) => {
        await run(`version`, `patch`);

        await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
          version: `0.0.0`,
        });

        await run(`version`, `apply`);

        await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
          version: `0.0.1`,
        });
      }),
    );

    test(
      `it should immediately increase the version number for a workspace when using --immediate, even if preferDeferredVersions is set`,
      makeTemporaryEnv({
        version: `0.0.0`,
      }, {
        preferDeferredVersions: true,
      }, async ({path, run, source}) => {
        await run(`version`, `patch`, `--immediate`);

        await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
          version: `0.0.1`,
        });
      }),
    );

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
      `it should throw when applying an invalid strategy`,
      makeTemporaryEnv({
        version: `1.0.0`,
      }, async ({path, run, source}) => {
        await expect(run(`version`, `invalid`)).rejects.toThrow(`invalid`);
      }),
    );

    test(
      `it should throw when applying an invalid strategy on top of the stored version`,
      makeTemporaryEnv({
        version: `1.0.0`,
      }, async ({path, run, source}) => {
        await run(`version`, `major`, `--deferred`);

        await expect(run(`version`, `invalid`)).rejects.toThrow(`invalid`);
      }),
    );

    test(
      `it should throw when applying an invalid strategy (deferred)`,
      makeTemporaryEnv({
        version: `1.0.0`,
      }, async ({path, run, source}) => {
        await expect(run(`version`, `invalid`, `--deferred`)).rejects.toThrow(`invalid`);
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
      `it should successfully apply a version bump that can't be described by a strategy`,
      makeTemporaryEnv({
        version: `1.0.0`,
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
  });
});
