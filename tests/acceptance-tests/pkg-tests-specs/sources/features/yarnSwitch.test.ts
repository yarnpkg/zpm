import {Filename, ppath, xfs} from '@yarnpkg/fslib';

describe(`Features`, () => {
  describe(`Yarn Switch`, () => {
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
