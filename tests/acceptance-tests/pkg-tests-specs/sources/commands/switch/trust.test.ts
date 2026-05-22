describe(`Commands`, () => {
  describe(`switch trust`, () => {
    test(
      `it should set and check project trust`,
      makeTemporaryEnv({}, async ({path, runSwitch}) => {
        await expect(runSwitch(`switch`, `trust`, `--check`, path)).rejects.toMatchObject({
          code: 3,
        });

        await expect(runSwitch(`switch`, `trust`, `--set`, `false`, path)).resolves.toMatchObject({
          code: 0,
        });

        await expect(runSwitch(`switch`, `trust`, `--check`, path)).rejects.toMatchObject({
          code: 2,
        });

        await expect(runSwitch(`switch`, `trust`, `--set`, `true`, path)).resolves.toMatchObject({
          code: 0,
        });

        await expect(runSwitch(`switch`, `trust`, `--check`, path)).resolves.toMatchObject({
          code: 0,
        });

        await expect(runSwitch(`switch`, `trust`, `--set`, `null`, path)).resolves.toMatchObject({
          code: 0,
        });

        await expect(runSwitch(`switch`, `trust`, `--check`, path)).rejects.toMatchObject({
          code: 3,
        });
      }),
    );

    test(
      `it should expose project trust through the cache listing`,
      makeTemporaryEnv({}, async ({path, runSwitch}) => {
        await runSwitch(`switch`, `trust`, `--set`, `true`, path);

        await expect(runSwitch(`switch`, `cache`)).resolves.toMatchObject({
          code: 0,
          stdout: expect.stringContaining(`Trusted: true`),
        });
      }),
    );

    test(
      `it should require trust before running install scripts through Yarn Switch`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps-scripted`]: `1.0.0`,
        },
      }, async ({path, runSwitch}) => {
        await expect(runSwitch(`install`)).rejects.toMatchObject({
          code: 1,
          stdout: expect.stringContaining(`must be trusted before Yarn can run install scripts`),
        });

        await runSwitch(`switch`, `trust`, `--set`, `true`, path);

        await expect(runSwitch(`install`)).resolves.toMatchObject({
          code: 0,
        });
      }),
    );

    test(
      `it should implicitly trust projects when running on CI`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps-scripted`]: `1.0.0`,
        },
      }, async ({path, runSwitch}) => {
        await expect(runSwitch(`switch`, `trust`, `--check`, path, {
          env: {CI: `1`},
        })).resolves.toMatchObject({
          code: 0,
        });

        await expect(runSwitch(`install`, {
          env: {CI: `1`},
        })).resolves.toMatchObject({
          code: 0,
        });
      }),
    );

    test(
      `it should still honor an explicit untrust on CI`,
      makeTemporaryEnv({}, async ({path, runSwitch}) => {
        await runSwitch(`switch`, `trust`, `--set`, `false`, path);

        await expect(runSwitch(`switch`, `trust`, `--check`, path, {
          env: {CI: `1`},
        })).rejects.toMatchObject({
          code: 2,
        });
      }),
    );
  });
});
