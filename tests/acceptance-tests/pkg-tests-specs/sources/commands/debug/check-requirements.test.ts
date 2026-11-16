describe(`Commands`, () => {
  describe(`debug check-requirements`, () => {
    for (const [requirements, os, expected] of [
      [{os: [`!foo`]}, `bar`, true],
      [{os: [`!foo`]}, `foo`, false],
      [{os: [`foo`, `!bar`]}, `foo`, true],
      [{os: [`foo`, `!bar`]}, `baz`, false],
      [{os: [`!foo`, `!bar`]}, `bar`, false],
    ] as const) {
      test(
        `it should report ${JSON.stringify(requirements)} as ${expected ? `valid` : `invalid`} on ${os}`,
        makeTemporaryEnv({}, async ({run}) => {
          const {stdout} = await run(`debug`, `check-requirements`, JSON.stringify(requirements), {
            env: {
              YARN_OS_OVERRIDE: os,
            },
          });

          expect(stdout).toContain(`Is valid system? ${expected}`);
        }),
      );
    }
  });
});
