import {npath, ppath, xfs} from '@yarnpkg/fslib';

// On Windows, Node always forwards USERPROFILE to child processes when it's
// missing from their environment, so we set the variables to empty values,
// which Yarn ignores.
const noHomeEnv = process.platform === `win32` ? {
  HOME: ``,
  USERPROFILE: ``,
} : {
  HOME: undefined,
  USERPROFILE: undefined,
};

describe(`Commands`, () => {
  describe(`switch`, () => {
    test(
      `it should show switch version`,
      makeTemporaryEnv({}, async ({path, runSwitch}) => {
        await expect(runSwitch(`switch`, `--version`)).resolves.toMatchObject({
          code: 0,
          stdout: expect.stringMatching(/^\d+\.\d+\.\d+/),
        });
      }),
    );

    test(
      `it should show switch which`,
      makeTemporaryEnv({}, async ({path, runSwitch}) => {
        await expect(runSwitch(`switch`, `which`)).resolves.toMatchObject({
          code: 0,
          stdout: expect.stringContaining(`yarn`),
        });
      }),
    );

    test(
      `it should download and run yarn when packageManager is set`,
      makeTemporaryEnv({
        packageManager: `yarn@6.0.0`,
      }, async ({path, runSwitch}) => {
        // This should trigger Yarn Switch to download the fake 6.0.0 release
        // and execute it with --version
        await expect(runSwitch(`--version`)).resolves.toMatchObject({
          code: 0,
          stdout: expect.stringContaining(`Fake Yarn 6.0.0`),
        });
      }),
    );

    test(
      `it should install yarn into YARNSW_CACHE_PATH without a home folder`,
      makeTemporaryEnv({
        packageManager: `yarn@6.0.0`,
      }, async ({path, runSwitch}) => {
        const cachePath = ppath.join(path, `switch-cache`);
        const env = {...noHomeEnv, YARNSW_CACHE_PATH: npath.fromPortablePath(cachePath)};

        // Doesn't execute the downloaded release, which is a shell script
        await expect(runSwitch(`switch`, `cache`, `--install`, {env})).resolves.toMatchObject({
          code: 0,
        });

        await expect(runSwitch(`switch`, `cache`, `--check`, `6.0.0`, {env})).resolves.toMatchObject({
          code: 0,
        });

        await expect(xfs.readdirPromise(cachePath)).resolves.toHaveLength(1);
      }),
    );

    test(
      `it should run yarn without a home folder`,
      makeTemporaryEnv({}, async ({path, runSwitch}) => {
        // Runs the local yarn-bin binary the test environment sets as default
        await expect(runSwitch(`--version`, {env: noHomeEnv})).resolves.toMatchObject({
          code: 0,
          stdout: expect.stringMatching(/^\d+\.\d+\.\d+/),
        });

        await expect(runSwitch(`switch`, `--version`, {env: noHomeEnv})).resolves.toMatchObject({
          code: 0,
          stdout: expect.stringMatching(/^\d+\.\d+\.\d+/),
        });
      }),
    );
  });
});
