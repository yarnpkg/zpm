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

    test(
      `it should print durations in a form that config set accepts`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        const env = {env: {YARN_NPM_MINIMAL_AGE_GATE: undefined}};

        await xfs.writeFilePromise(`${path}/.yarnrc.yml`, [
          `npmMinimalAgeGate: 72h\n`,
          `pypiMinimalAgeGate: 0\n`,
          `packageRules:\n`,
          `  - packageFilter: lodash\n`,
          `    npmMinimalAgeGate: 90s\n`,
        ].join(``));

        await expect(run(`config`, `get`, `npmMinimalAgeGate`, env)).resolves.toMatchObject({
          stdout: `3d\n`,
        });

        await expect(run(`config`, `get`, `--json`, `npmMinimalAgeGate`, env)).resolves.toMatchObject({
          stdout: `"3d"\n`,
        });

        await expect(run(`config`, `get`, `pypiMinimalAgeGate`, env)).resolves.toMatchObject({
          stdout: `0s\n`,
        });

        const {stdout: rules} = await run(`config`, `get`, `--json`, `packageRules`, env);
        expect(JSON.parse(rules)).toMatchObject([{packageFilter: `lodash`, npmMinimalAgeGate: `90s`}]);

        const {stdout: listing} = await run(`config`, `--json`, env);
        expect(listing.split(`\n`).filter(line => line).map(line => JSON.parse(line))).toEqual(expect.arrayContaining([
          {key: `npmMinimalAgeGate`, effective: `3d`, source: `<project>`},
          {key: `pypiMinimalAgeGate`, effective: `0s`, source: `<project>`},
        ]));

        await expect(run(`config`, env)).resolves.toMatchObject({
          stdout: expect.stringMatching(/npmMinimalAgeGate\n.*\n.*Value: 3d\n/),
        });
      }),
    );
  });
});
