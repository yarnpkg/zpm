import {PortablePath, xfs} from '@yarnpkg/fslib';

describe(`Commands`, () => {
  describe(`config unset`, () => {
    test(
      `it should unset a simple config correctly`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`config`, `set`, `pnpShebang`, `#!/usr/bin/env iojs\n`);
        await expect(xfs.readFilePromise(`${path}/.yarnrc.yml` as PortablePath, `utf8`)).resolves.toContain(`pnpShebang`);
        await expect(run(`config`, `unset`, `pnpShebang`)).resolves.toMatchObject({
          stdout: expect.stringContaining(`Successfully unset`),
        });
        await expect(xfs.readFilePromise(`${path}/.yarnrc.yml` as PortablePath, `utf8`)).resolves.toStrictEqual(``);
      }),
    );

    test(
      `it should unset a complex config correctly`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`config`, `set`, `npmScopes.yarnpkg`, `--json`, JSON.stringify({
          npmAlwaysAuth: false,
        }));
        await expect(xfs.readFilePromise(`${path}/.yarnrc.yml` as PortablePath, `utf8`)).resolves.toContain(`npmScopes`);
        await expect(run(`config`, `unset`, `npmScopes`)).resolves.toMatchObject({
          stdout: expect.stringContaining(`Successfully unset`),
        });
        await expect(xfs.readFilePromise(`${path}/.yarnrc.yml` as PortablePath, `utf8`)).resolves.toStrictEqual(``);
      }),
    );

    test(
      `it should unset a nested config correctly`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`config`, `set`, `npmScopes.yarnpkg`, `--json`, JSON.stringify({
          npmRegistryServer: `https://registry.yarnpkg.com`,
          npmAlwaysAuth: false,
        }));
        await expect(xfs.readFilePromise(`${path}/.yarnrc.yml` as PortablePath, `utf8`)).resolves.toContain(`npmScopes`);
        await expect(run(`config`, `unset`, `npmScopes.yarnpkg.npmAlwaysAuth`)).resolves.toMatchObject({
          stdout: expect.stringContaining(`Successfully unset`),
        });
        await expect(xfs.readFilePromise(`${path}/.yarnrc.yml` as PortablePath, `utf8`)).resolves.toContain(`npmScopes`);
        await expect(xfs.readFilePromise(`${path}/.yarnrc.yml` as PortablePath, `utf8`)).resolves.toContain(`npmRegistryServer`);
        await expect(xfs.readFilePromise(`${path}/.yarnrc.yml` as PortablePath, `utf8`)).resolves.not.toContain(`npmAlwaysAuth`);
      }),
    );

    test(
      `it should show a warning when unsetting a simple config setting that's not set`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await expect(run(`config`, `unset`, `pnpShebang`)).resolves.toMatchObject({
          stdout: expect.stringContaining(`Configuration doesn't contain setting pnpShebang; there is nothing to unset`),
        });
      }),
    );

    test(
      `it should show a warning when unsetting a nested config setting that's not set`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`config`, `set`, `npmScopes.yarnpkg`, `--json`, JSON.stringify({
          npmRegistryServer: `https://registry.yarnpkg.com`,
        }));
        await expect(run(`config`, `unset`, `npmScopes.yarnpkg.npmAlwaysAuth`)).resolves.toMatchObject({
          stdout: expect.stringContaining(`Configuration doesn't contain setting npmScopes.yarnpkg.npmAlwaysAuth; there is nothing to unset`),
        });
      }),
    );

    test(
      `it should allow running the command from arbitrary folders if the -H,--home option is set`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        const tmpDir = await xfs.mktempPromise();

        await expect(run(`config`, `unset`, `--home`, `pnpShebang`, {cwd: tmpDir})).resolves.toMatchObject({
          code: 0,
        });
      }),
    );

    test(
      `it should remove the entries left empty by unsetting their last key`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`config`, `set`, `npmScopes.foo.npmAlwaysAuth`, `true`);
        await run(`config`, `set`, `npmScopes.bar.npmAlwaysAuth`, `false`);
        await run(`config`, `set`, `npmRegistries["https://example.org"].npmAuthToken`, `secret`);

        await run(`config`, `unset`, `npmScopes.foo.npmAlwaysAuth`);

        await expect(xfs.readFilePromise(`${path}/.yarnrc.yml` as PortablePath, `utf8`)).resolves.toEqual([
          `npmRegistries:\n`,
          `  "https://example.org":\n`,
          `    npmAuthToken: secret\n`,
          `npmScopes:\n`,
          `  bar:\n`,
          `    npmAlwaysAuth: false\n`,
        ].join(``));

        await run(`config`, `unset`, `npmRegistries["https://example.org"].npmAuthToken`);
        await run(`config`, `unset`, `npmScopes.bar.npmAlwaysAuth`);

        await expect(xfs.readFilePromise(`${path}/.yarnrc.yml` as PortablePath, `utf8`)).resolves.toEqual(``);
      }),
    );

    test(
      `it should unset durations and awkward keys written by config set`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await xfs.writeFilePromise(`${path}/.yarnrc.yml` as PortablePath, `# Keep me\nenableTelemetry: false\n`);

        await run(`config`, `set`, `npmMinimalAgeGate`, `3h`);
        await run(`config`, `set`, `packageExtensions`, `--json`, JSON.stringify({[`@scoped/no-deps@*`]: {dependencies: {[`no-deps`]: `*`}}}));

        await run(`config`, `unset`, `packageExtensions["@scoped/no-deps@*"]`);
        await run(`config`, `unset`, `npmMinimalAgeGate`);

        await expect(xfs.readFilePromise(`${path}/.yarnrc.yml` as PortablePath, `utf8`)).resolves.toEqual(`# Keep me\nenableTelemetry: false\n`);
      }),
    );

    test(
      `it should unset nested entries from the home configuration`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        const tmpDir = await xfs.mktempPromise();
        const homeRc = `${path}/../.yarnrc.yml` as PortablePath;

        await run(`config`, `set`, `-H`, `npmScopes.foo.npmAlwaysAuth`, `true`, {cwd: tmpDir});
        await expect(xfs.readFilePromise(homeRc, `utf8`)).resolves.toEqual(`npmScopes:\n  foo:\n    npmAlwaysAuth: true\n`);

        await run(`config`, `unset`, `-H`, `npmScopes.foo.npmAlwaysAuth`, {cwd: tmpDir});
        await expect(xfs.readFilePromise(homeRc, `utf8`)).resolves.toEqual(``);
      }),
    );
  });
});
