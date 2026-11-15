import {Filename, PortablePath, ppath, xfs} from '@yarnpkg/fslib';

const {
  tests: {testIf},
} = require(`pkg-tests-core`);

describe(`Features`, () => {
  describe(`Yarn Switch`, () => {
    describe(`project package manager`, () => {
      for (const packageManager of [undefined, `yarn@6.0.0`]) {
        for (const version of [undefined, `6.0.0`, `5.0.0`]) {
          test(
            `it should reject devEngines.packageManager.name=yarn (version=${version}) with packageManager=${packageManager}`,
            makeTemporaryEnv({
              packageManager,
              devEngines: {packageManager: {name: `yarn`, version}},
            }, async ({runSwitch}) => {
              await expect(runSwitch(`--version`)).rejects.toMatchObject({
                code: 1,
                stdout: expect.stringMatching(/Yarn can't be selected through devEngines\.packageManager .*"packageManager": "yarn@<version>"/),
              });
            }),
          );
        }

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

      for (const onFail of [`ignore`, `warn`, `error`]) {
        test(
          `it should reject a Yarn devEngines entry even with onFail=${onFail}`,
          makeTemporaryEnv({
            devEngines: {packageManager: {name: `yarn`, version: `6.0.0`, onFail}},
          }, async ({runSwitch}) => {
            await expect(runSwitch(`--version`)).rejects.toMatchObject({
              code: 1,
              stdout: expect.stringContaining(`Yarn can't be selected through devEngines.packageManager`),
            });
          }),
        );
      }

      for (const names of [[`yarn`], [`yarn`, `pnpm`], [`npm`, `yarn`]]) {
        test(
          `it should reject a Yarn entry in a devEngines.packageManager array (${names.join(`, `)})`,
          makeTemporaryEnv({
            packageManager: `yarn@6.0.0`,
            devEngines: {packageManager: names.map(name => ({name}))},
          }, async ({runSwitch}) => {
            await expect(runSwitch(`--version`)).rejects.toMatchObject({
              code: 1,
              stdout: expect.stringContaining(`Yarn can't be selected through devEngines.packageManager`),
            });
          }),
        );
      }

      test(
        `it should reject a devEngines.packageManager array that only lists other managers`,
        makeTemporaryEnv({
          packageManager: `yarn@6.0.0`,
          devEngines: {packageManager: [{name: `npm`}, {name: `pnpm`}]},
        }, async ({runSwitch}) => {
          await expect(runSwitch(`--version`)).rejects.toMatchObject({
            code: 1,
            stdout: expect.stringContaining(`configured for use with npm, pnpm (devEngines.packageManager.name)`),
          });
        }),
      );

      test(
        `it should report the top-level packageManager when it names another manager next to a Yarn devEngines array entry`,
        makeTemporaryEnv({
          packageManager: `pnpm@10.0.0`,
          devEngines: {packageManager: [{name: `yarn`}]},
        }, async ({runSwitch}) => {
          await expect(runSwitch(`--version`)).rejects.toMatchObject({
            code: 1,
            stdout: expect.stringContaining(`configured for use with pnpm (packageManager)`),
          });
        }),
      );

      test(
        `it should reject a devEngines-only Yarn declaration from a subdirectory`,
        makeTemporaryEnv({
          devEngines: {packageManager: {name: `yarn`, version: `6.0.0`}},
        }, async ({path, runSwitch}) => {
          const cwd = ppath.join(path, `packages`, `child`);
          await xfs.mkdirpPromise(cwd);
          await xfs.writeJsonPromise(ppath.join(cwd, Filename.manifest), {});

          await expect(runSwitch(`--version`, {cwd})).rejects.toMatchObject({
            code: 1,
            stdout: expect.stringContaining(`Yarn can't be selected through devEngines.packageManager`),
          });
        }),
      );

      test(
        `it should stop at a devEngines-only Yarn declaration before an ancestor packageManager`,
        makeTemporaryEnv({
          packageManager: `yarn@6.0.0`,
        }, async ({path, runSwitch}) => {
          const cwd = ppath.join(path, `child`);
          await xfs.mkdirpPromise(cwd);
          await xfs.writeJsonPromise(ppath.join(cwd, Filename.manifest), {
            devEngines: {packageManager: {name: `yarn`, version: `6.0.0`}},
          });

          await expect(runSwitch(`--version`, {cwd})).rejects.toMatchObject({
            code: 1,
            stdout: expect.stringContaining(`Yarn can't be selected through devEngines.packageManager`),
          });
        }),
      );

      for (const devEngines of [
        `yarn@6.0.0`,
        [],
        {packageManager: `yarn@6.0.0`},
        {packageManager: {name: 42}},
        {packageManager: [{name: 42}, `yarn`, [`yarn`]]},
      ]) {
        testIf(
          `runnableFakeYarnRelease`,
          `it should ignore a malformed devEngines field (${JSON.stringify(devEngines)})`,
          makeTemporaryEnv({
            packageManager: `yarn@6.0.0`,
            devEngines,
          }, async ({runSwitch}) => {
            await expect(runSwitch(`--version`)).resolves.toMatchObject({
              code: 0,
              stdout: expect.stringContaining(`Fake Yarn 6.0.0`),
            });
          }),
        );
      }

      for (const name of [`yarn`, `pnpm`]) {
        const expectedError = name === `yarn`
          ? `Yarn can't be selected through devEngines.packageManager`
          : `configured for use with pnpm (devEngines.packageManager.name)`;

        const makeNestedEnv = (fn: (args: {cwd: PortablePath, runSwitch: any}) => Promise<void>) => makeTemporaryEnv({
          packageManager: `yarn@6.0.0`,
        }, async ({path, runSwitch}) => {
          const cwd = ppath.join(path, `child`);
          await xfs.mkdirpPromise(cwd);
          await xfs.writeJsonPromise(ppath.join(cwd, Filename.manifest), {
            devEngines: {packageManager: {name}},
          });

          await fn({cwd, runSwitch});
        });

        test(
          `it should reject yarn init under a nested devEngines ${name} declaration`,
          makeNestedEnv(async ({cwd, runSwitch}) => {
            await expect(runSwitch(`init`, {cwd})).rejects.toMatchObject({
              code: 1,
              stdout: expect.stringContaining(expectedError),
            });

            await expect(xfs.readJsonPromise(ppath.join(cwd, Filename.manifest))).resolves.toEqual({
              devEngines: {packageManager: {name}},
            });
          }),
        );

        test(
          `it should refuse to start a daemon under a nested devEngines ${name} declaration`,
          makeNestedEnv(async ({cwd, runSwitch}) => {
            await expect(runSwitch(`switch`, `daemon`, `--start`, {cwd})).rejects.toMatchObject({
              code: 1,
              stdout: expect.stringContaining(expectedError),
            });
          }),
        );

        test(
          `it should refuse to install the project version under a nested devEngines ${name} declaration`,
          makeNestedEnv(async ({cwd, runSwitch}) => {
            await expect(runSwitch(`switch`, `cache`, `--install`, {cwd})).rejects.toMatchObject({
              code: 1,
              stdout: expect.stringContaining(expectedError),
            });
          }),
        );
      }

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

      for (const link of [`local`, `migration`]) {
        test(
          `it should reject a Yarn devEngines entry before applying a ${link} link`,
          makeTemporaryEnv({
            packageManager: `yarn@6.0.0`,
            packageManagerMigration: `yarn@6.0.0`,
            devEngines: {packageManager: {name: `yarn`, version: `6.0.0`}},
          }, async ({runSwitch, yarnBinary}) => {
            if (link === `local`)
              await runSwitch(`switch`, `link`, yarnBinary);
            else
              await runSwitch(`switch`, `link`, `--migration`);

            try {
              await expect(runSwitch(`--version`)).rejects.toMatchObject({
                code: 1,
                stdout: expect.stringContaining(`Yarn can't be selected through devEngines.packageManager`),
              });
            } finally {
              await runSwitch(`switch`, `unlink`);
            }
          }),
        );
      }

      testIf(
        `runnableFakeYarnRelease`,
        `it should allow an explicit Yarn selector to override a Yarn devEngines entry`,
        makeTemporaryEnv({
          devEngines: {packageManager: {name: `yarn`, version: `5.0.0`}},
        }, async ({runSwitch}) => {
          await expect(runSwitch(`switch`, `6.0.0`, `--version`)).resolves.toMatchObject({
            code: 0,
            stdout: expect.stringContaining(`Fake Yarn 6.0.0`),
          });
        }),
      );

      testIf(
        `runnableFakeYarnRelease`,
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
