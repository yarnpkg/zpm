import {Filename, ppath, xfs} from '@yarnpkg/fslib';

describe(`Features`, () => {
  describe(`Yarn Switch`, () => {
    describe(`project package manager`, () => {
      for (const packageManager of [undefined, `yarn@6.0.0`]) {
        test(
          `it should accept devEngines.packageManager.name=yarn with packageManager=${packageManager}`,
          makeTemporaryEnv({
            packageManager,
            devEngines: {packageManager: {name: `yarn`}},
          }, async ({runSwitch}) => {
            await expect(runSwitch(`--version`)).resolves.toMatchObject({code: 0});
          }),
        );

        for (const name of [`npm`, `pnpm`]) {
          test(
            `it should reject devEngines.packageManager.name=${name} with packageManager=${packageManager}`,
            makeTemporaryEnv({
              packageManager,
              devEngines: {packageManager: {name}},
            }, async ({runSwitch}) => {
              await expect(runSwitch(`--version`)).rejects.toMatchObject({
                code: 1,
                stdout: expect.stringContaining(`configured for use with ${name} (devEngines.packageManager.name)`),
              });
            }),
          );
        }
      }

      test(
        `it should report the actual manager from the top-level packageManager field`,
        makeTemporaryEnv({
          packageManager: `pnpm@10.0.0`,
          devEngines: {packageManager: {name: `yarn`}},
        }, async ({runSwitch}) => {
          await expect(runSwitch(`--version`)).rejects.toMatchObject({
            code: 1,
            stdout: expect.stringContaining(`configured for use with pnpm (packageManager)`),
          });
        }),
      );

      test(
        `it should allow devEngines without a package manager name`,
        makeTemporaryEnv({
          devEngines: {packageManager: {version: `>=6.0.0`}},
        }, async ({runSwitch}) => {
          await expect(runSwitch(`--version`)).resolves.toMatchObject({code: 0});
        }),
      );

      test(
        `it should find a devEngines-only declaration from a subdirectory`,
        makeTemporaryEnv({
          devEngines: {packageManager: {name: `pnpm`}},
        }, async ({path, runSwitch}) => {
          const cwd = ppath.join(path, `packages`, `child`);
          await xfs.mkdirpPromise(cwd);
          await xfs.writeJsonPromise(ppath.join(cwd, Filename.manifest), {});

          await expect(runSwitch(`--version`, {cwd})).rejects.toMatchObject({
            code: 1,
            stdout: expect.stringContaining(`configured for use with pnpm (devEngines.packageManager.name)`),
          });
        }),
      );

      test(
        `it should stop at a devEngines-only declaration before an ancestor packageManager`,
        makeTemporaryEnv({
          packageManager: `yarn@6.0.0`,
        }, async ({path, runSwitch}) => {
          const cwd = ppath.join(path, `child`);
          await xfs.mkdirpPromise(cwd);
          await xfs.writeJsonPromise(ppath.join(cwd, Filename.manifest), {
            devEngines: {packageManager: {name: `pnpm`}},
          });

          await expect(runSwitch(`--version`, {cwd})).rejects.toMatchObject({
            code: 1,
            stdout: expect.stringContaining(`configured for use with pnpm (devEngines.packageManager.name)`),
          });
        }),
      );

      test(
        `it should respect a nearer Yarn lockfile boundary`,
        makeTemporaryEnv({
          devEngines: {packageManager: {name: `pnpm`}},
        }, async ({path, runSwitch}) => {
          const cwd = ppath.join(path, `child`);
          await xfs.mkdirpPromise(cwd);
          await xfs.writeJsonPromise(ppath.join(cwd, Filename.manifest), {});
          await xfs.writeFilePromise(ppath.join(cwd, Filename.lockfile), ``);

          await expect(runSwitch(`--version`, {cwd})).resolves.toMatchObject({code: 0});
        }),
      );

      for (const onFail of [`warn`, `ignore`]) {
        test(
          `it should reject a non-Yarn manager even with onFail=${onFail}`,
          makeTemporaryEnv({
            devEngines: {packageManager: {name: `pnpm`, onFail}},
          }, async ({runSwitch}) => {
            await expect(runSwitch(`--version`)).rejects.toMatchObject({
              code: 1,
              stdout: expect.stringContaining(`devEngines.packageManager.name`),
            });
          }),
        );
      }

      for (const link of [`local`, `migration`]) {
        test(
          `it should reject a non-Yarn devEngines manager before applying a ${link} link`,
          makeTemporaryEnv({
            packageManager: `yarn@6.0.0`,
            packageManagerMigration: `yarn@6.0.0`,
            devEngines: {packageManager: {name: `pnpm`}},
          }, async ({runSwitch, yarnBinary}) => {
            if (link === `local`)
              await runSwitch(`switch`, `link`, yarnBinary);
            else
              await runSwitch(`switch`, `link`, `--migration`);

            try {
              await expect(runSwitch(`--version`)).rejects.toMatchObject({
                code: 1,
                stdout: expect.stringContaining(`devEngines.packageManager.name`),
              });
            } finally {
              await runSwitch(`switch`, `unlink`);
            }
          }),
        );
      }

      test(
        `it should allow an explicit Yarn selector to override the project manager`,
        makeTemporaryEnv({
          packageManager: `pnpm@10.0.0`,
          devEngines: {packageManager: {name: `pnpm`}},
        }, async ({runSwitch}) => {
          await expect(runSwitch(`switch`, `6.0.0`, `--version`)).resolves.toMatchObject({
            code: 0,
            stdout: expect.stringContaining(`Fake Yarn 6.0.0`),
          });
        }),
      );
    });

    describe(`switchVersionRequirement`, () => {
      test(
        `it should succeed when no config file exists`,
        makeTemporaryEnv({
          packageManager: `yarn@6.0.0`,
        }, async ({path, runSwitch}) => {
          await expect(runSwitch(`--version`)).resolves.toMatchObject({
            code: 0,
          });
        }),
      );

      test(
        `it should succeed when the config file is empty`,
        makeTemporaryEnv({
          packageManager: `yarn@6.0.0`,
        }, async ({path, runSwitch}) => {
          const homePath = ppath.dirname(path);

          await xfs.writeFilePromise(ppath.join(homePath, Filename.rc), ``);

          await expect(runSwitch(`--version`)).resolves.toMatchObject({
            code: 0,
          });
        }),
      );

      test(
        `it should succeed when the version matches the requirement`,
        makeTemporaryEnv({
          packageManager: `yarn@6.0.0`,
        }, async ({path, runSwitch}) => {
          const homePath = ppath.dirname(path);

          await xfs.writeJsonPromise(ppath.join(homePath, Filename.rc), {
            switchVersionRequirement: `>=6.0.0`,
          });

          await expect(runSwitch(`--version`)).resolves.toMatchObject({
            code: 0,
          });
        }),
      );

      test(
        `it should fail when the version does not match the requirement`,
        makeTemporaryEnv({
          packageManager: `yarn@6.0.0`,
        }, async ({path, runSwitch}) => {
          const homePath = ppath.dirname(path);

          await xfs.writeJsonPromise(ppath.join(homePath, Filename.rc), {
            switchVersionRequirement: `>=99.0.0`,
          });

          await expect(runSwitch(`--version`)).rejects.toMatchObject({
            code: 1,
            stdout: expect.stringContaining(`does not satisfy the required range`),
          });
        }),
      );

      test(
        `it should fail to start a daemon when the version does not match the requirement`,
        makeTemporaryEnv({
          packageManager: `yarn@6.0.0`,
        }, async ({path, runSwitch}) => {
          const homePath = ppath.dirname(path);

          await xfs.writeJsonPromise(ppath.join(homePath, Filename.rc), {
            switchVersionRequirement: `>=99.0.0`,
          });

          await expect(runSwitch(`switch`, `daemon`, `--start`)).rejects.toMatchObject({
            code: 1,
            stdout: expect.stringContaining(`does not satisfy the required range`),
          });
        }),
      );
    });
  });
});
