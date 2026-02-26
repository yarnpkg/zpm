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
  });
});
