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
      `it should cache downloaded versions`,
      makeTemporaryEnv({
        packageManager: `yarn@6.0.0`,
      }, async ({path, runSwitch}) => {
        // First run should download
        await expect(runSwitch(`--version`)).resolves.toMatchObject({
          code: 0,
        });

        // Second run should use cache (same result, but faster)
        await expect(runSwitch(`--version`)).resolves.toMatchObject({
          code: 0,
          stdout: expect.stringContaining(`Fake Yarn 6.0.0`),
        });
      }),
    );

    test(
      `it should show cache list`,
      makeTemporaryEnv({
        packageManager: `yarn@6.0.0`,
      }, async ({path, runSwitch}) => {
        // Download a version first
        await runSwitch(`--version`);

        // Check cache list includes the downloaded version
        await expect(runSwitch(`switch`, `cache`)).resolves.toMatchObject({
          code: 0,
          stdout: expect.stringContaining(`6.0.0`),
        });
      }),
    );

    test(
      `it should clear cache`,
      makeTemporaryEnv({
        packageManager: `yarn@6.0.0`,
      }, async ({path, runSwitch}) => {
        // Download a version first
        await runSwitch(`--version`);

        // Clear the cache
        await expect(runSwitch(`switch`, `cache`, `--clear`)).resolves.toMatchObject({
          code: 0,
        });

        // Cache list should now be empty (or show no versions)
        const result = await runSwitch(`switch`, `cache`);
        expect(result.code).toBe(0);
        // After clearing, either empty or no 6.0.0
        expect(result.stdout).not.toContain(`6.0.0`);
      }),
    );
  });
});
