import {npath, xfs} from '@yarnpkg/fslib';

describe(`Commands`, () => {
  describe(`config get`, () => {
    test(
      `it should print the requested configuration value for the current directory`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await expect(run(`config`, `get`, `pnpShebang`)).resolves.toMatchObject({
          stdout: `#!/usr/bin/env node\n`,
        });
      }),
    );

    test(
      `it shouldn't print secrets by default`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await xfs.writeFilePromise(`${path}/.yarnrc.yml`, `npmAuthToken: foobar\n`);

        await expect(run(`config`, `get`, `npmAuthToken`)).resolves.toMatchObject({
          stdout: `<redacted>\n`,
        });
      }),
    );

    test(
      `it should print secrets when using the --no-redacted flag`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await xfs.writeFilePromise(`${path}/.yarnrc.yml`, `npmAuthToken: foobar\n`);

        await expect(run(`config`, `get`, `npmAuthToken`, `--no-redacted`)).resolves.toMatchObject({
          stdout: `foobar\n`,
        });
      }),
    );

    test(
      `it should print native paths`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        const {stdout} = await run(`config`, `get`, `cacheFolder`, `--no-redacted`);
        const value = stdout.trim();

        expect(value).toEqual(npath.fromPortablePath(value));
      }),
    );

    test(
      `it should print cloneConcurrency default value`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await expect(run(`config`, `get`, `--json`, `cloneConcurrency`)).resolves.toMatchObject({
          stdout: `2\n`,
        });
      }),
    );

    test(
      `it should print cloneConcurrency configured value`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await xfs.writeFilePromise(`${path}/.yarnrc.yml`, `cloneConcurrency: 7\n`);

        await expect(run(`config`, `get`, `--json`, `cloneConcurrency`)).resolves.toMatchObject({
          stdout: `7\n`,
        });
      }),
    );

    test(
      `it should reject cloneConcurrency lower than 1`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await xfs.writeFilePromise(`${path}/.yarnrc.yml`, `cloneConcurrency: 0\n`);

        await expect(run(`config`, `get`, `cloneConcurrency`)).rejects.toMatchObject({
          stdout: expect.stringContaining(`Invalid config value for cloneConcurrency (must be >= 1)`),
        });
      }),
    );

    test(
      `it should support printing sub-keys`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await xfs.writeFilePromise(`${path}/.yarnrc.yml`, `packageExtensions:\n  "foo@*":\n    dependencies:\n      "bar": "1.0.0"\n`);

        await expect(run(`config`, `get`, `packageExtensions["foo@*"].dependencies["bar"]`)).resolves.toMatchObject({
          stdout: `1.0.0\n`,
        });
      }),
    );
  });
});
