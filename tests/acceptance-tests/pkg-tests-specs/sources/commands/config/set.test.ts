import {Filename, ppath, xfs} from '@yarnpkg/fslib';
import {parseSyml}            from '@yarnpkg/parsers';
import {EOL}                  from 'os';
import {tests}                from 'pkg-tests-core';

// The test environment sets these settings through environment variables,
// which would take precedence over what `yarn config set` writes
const WITHOUT_ENV_OVERRIDES = {
  env: {
    YARN_NPM_MINIMAL_AGE_GATE: undefined,
    YARN_DAEMON_DEFAULT_WARMUP_PERIOD: undefined,
  },
};

const readRc = (path: any) => xfs.readFilePromise(ppath.join(path, Filename.rc), `utf8`);

// Values that YAML would read as something else than the original string if
// they weren't quoted (or that would break the document)
const AWKWARD_STRINGS = [
  `@scope/name`, `*foo`, `&anchor`, `!tag`, `%directive`, `\`backtick`, `|block`, `>folded`, `? question`,
  `[bracket`, `{brace`, `#hash`, `a #comment`, `a: b`, `https://example.org`, ` leading`, `trailing `,
  `1.0`, `1e3`, `0x10`, `.inf`, `true`, `False`, `null`, `~`, `no`, `yes`, `off`, `multi\nline`,
  `it's`, `"quoted"`, `back\\slash`, `tab\tchar`,
];

describe(`Commands`, () => {
  describe(`config set`, () => {
    test(
      `it shouldn't remove empty arrays from the files`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
          injectEnvironmentFiles: [],
        });

        await run(`config`, `set`, `pnpShebang`, `#!/usr/bin/env iojs\n`);

        expect(parseSyml(await xfs.readFilePromise(ppath.join(path, Filename.rc), `utf8`))).toMatchObject({
          injectEnvironmentFiles: [],
        });
      }),
    );

    test(
      `it should print the configured value for the current directory`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await expect(run(`config`, `set`, `pnpShebang`, `#!/usr/bin/env iojs\n`)).resolves.toMatchObject({
          stdout: expect.stringContaining(`#!/usr/bin/env iojs\\n`),
        });

        await expect(xfs.readFilePromise(ppath.join(path, Filename.rc), `utf8`)).resolves.toContain(`pnpShebang`);
      }),
    );

    test(
      `it shouldn't print secrets`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        const {stdout} = await run(`config`, `set`, `npmAuthToken`, `foobar`);

        expect(stdout).not.toContain(`foobar`);
        expect(stdout).toContain(`<redacted>`);

        await expect(xfs.readFilePromise(ppath.join(path, Filename.rc), `utf8`)).resolves.toContain(`npmAuthToken: foobar`);
      }),
    );

    test(
      `it should update registry entries keyed without the protocol in place`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
          npmRegistries: {
            [`//npm.pkg.github.com`]: {
              npmAlwaysAuth: true,
            },
          },
        });

        await run(`config`, `set`, `npmRegistries["//npm.pkg.github.com"].npmAuthToken`, `foobar`);

        expect(parseSyml(await xfs.readFilePromise(ppath.join(path, Filename.rc), `utf8`))).toStrictEqual({
          npmRegistries: {
            [`//npm.pkg.github.com`]: {
              npmAlwaysAuth: `true`, // parseSyml doesn't parse booleans
              npmAuthToken: `foobar`,
            },
          },
        });

        await run(`config`, `unset`, `npmRegistries["//npm.pkg.github.com"].npmAuthToken`);

        expect(parseSyml(await xfs.readFilePromise(ppath.join(path, Filename.rc), `utf8`))).toStrictEqual({
          npmRegistries: {
            [`//npm.pkg.github.com`]: {
              npmAlwaysAuth: `true`, // parseSyml doesn't parse booleans
            },
          },
        });
      }),
    );

    test(
      `it should support setting JSON values`,
      makeTemporaryEnv({enableColors: false}, async({path, run, source}) => {
        const {stdout} = await run(`config`, `set`, `npmScopes.yarnpkg`, `--json`, JSON.stringify({
          npmAlwaysAuth: false,
        }));

        expect(stdout).toContain(`npmScopes.yarnpkg`);
        expect(stdout).toContain(`npmAlwaysAuth: false`);

        await expect(xfs.readFilePromise(ppath.join(path, Filename.rc), `utf8`)).resolves.toContain(
          `npmScopes:${EOL}` +
          `  yarnpkg:${EOL}` +
          `    npmAlwaysAuth: false`,
        );
      }),
    );

    test(
      `it should reject invalid JSON values and not corrupt the config file`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`config`, `set`, `pnpShebang`, `#!/usr/bin/env iojs\n`);

        await expect(run(`config`, `set`, `enableColors`, `--json`, JSON.stringify(`not-a-bool`))).rejects.toMatchObject({
          code: 1,
        });

        expect(parseSyml(await xfs.readFilePromise(ppath.join(path, Filename.rc), `utf8`))).toMatchObject({
          pnpShebang: `#!/usr/bin/env iojs\n`,
        });
      }),
    );

    test(
      `it should reject package rules without filters`,
      makeTemporaryEnv({}, async ({run}) => {
        await expect(run(`config`, `set`, `packageRules`, `--json`, JSON.stringify([{
          npmAlwaysAuth: true,
        }]))).rejects.toMatchObject({
          code: 1,
        });
      }),
    );

    test(
      `it should reject source rules without filters`,
      makeTemporaryEnv({}, async ({run}) => {
        await expect(run(`config`, `set`, `sourceRules`, `--json`, JSON.stringify([{
          npmAlwaysAuth: true,
        }]))).rejects.toMatchObject({
          code: 1,
        });
      }),
    );

    test(
      `it should allow running the command from arbitrary folders if the -H,--home option is set`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        const tmpDir = await xfs.mktempPromise();

        await expect(run(`config`, `set`, `--home`, `pnpShebang`, `#!/usr/bin/env iojs\n`, {cwd: tmpDir})).resolves.toMatchObject({
          code: 0,
        });
      }),
    );

    const durations: Array<[string, string, string]> = [
      [`npmMinimalAgeGate`, `3h`, `3h`],
      [`npmMinimalAgeGate`, `0`, `0s`],
      [`npmMinimalAgeGate`, `1.5h`, `90m`],
      [`npmMinimalAgeGate`, `86400s`, `1d`],
      [`pypiMinimalAgeGate`, `2d`, `2d`],
      [`daemonDefaultWarmupPeriod`, `1500ms`, `1500ms`],
    ];

    for (const [name, input, expected] of durations) {
      test(
        `it should write durations with an explicit unit (${name}, ${input})`,
        makeTemporaryEnv({}, async ({path, run, source}) => {
          await expect(run(`config`, `set`, name, input, WITHOUT_ENV_OVERRIDES)).resolves.toMatchObject({
            stdout: expect.stringContaining(`Successfully set ${name} to "${expected}"`),
          });

          await expect(readRc(path)).resolves.toEqual(`${name}: ${expected}\n`);

          await expect(run(`config`, `get`, name, WITHOUT_ENV_OVERRIDES)).resolves.toMatchObject({
            stdout: `${expected}\n`,
          });

          await expect(run(`config`, `get`, `--json`, name, WITHOUT_ENV_OVERRIDES)).resolves.toMatchObject({
            stdout: `"${expected}"\n`,
          });

          // What `config get` prints must be accepted back
          await run(`config`, `set`, name, expected, WITHOUT_ENV_OVERRIDES);
          await expect(readRc(path)).resolves.toEqual(`${name}: ${expected}\n`);
        }),
      );
    }

    test(
      `it should write durations with an explicit unit when using --json`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`config`, `set`, `npmMinimalAgeGate`, `--json`, `"180m"`, WITHOUT_ENV_OVERRIDES);
        await expect(readRc(path)).resolves.toEqual(`npmMinimalAgeGate: 3h\n`);
      }),
    );

    test(
      `it should write durations nested in package and source rules as strings`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`config`, `set`, `packageRules`, `--json`, JSON.stringify([{
          packageFilter: `release-date`,
          npmMinimalAgeGate: `1d`,
        }]), WITHOUT_ENV_OVERRIDES);

        await run(`config`, `set`, `sourceRules`, `--json`, JSON.stringify([{
          ecosystemFilter: `pypi`,
          pypiMinimalAgeGate: `3h`,
        }]), WITHOUT_ENV_OVERRIDES);

        await expect(readRc(path)).resolves.toEqual([
          `packageRules:\n`,
          `  - packageFilter: release-date\n`,
          `    npmMinimalAgeGate: 1d\n`,
          `sourceRules:\n`,
          `  - ecosystemFilter: pypi\n`,
          `    pypiMinimalAgeGate: 3h\n`,
        ].join(``));

        const {stdout} = await run(`config`, `get`, `--json`, `packageRules`, WITHOUT_ENV_OVERRIDES);
        expect(JSON.parse(stdout)).toMatchObject([{packageFilter: `release-date`, npmMinimalAgeGate: `1d`}]);

        // The output of `config get --json` must be accepted back
        await run(`config`, `set`, `packageRules`, `--json`, stdout, WITHOUT_ENV_OVERRIDES);
        await expect(run(`config`, `get`, `--json`, `packageRules`, WITHOUT_ENV_OVERRIDES)).resolves.toMatchObject({stdout});
      }),
    );

    test(
      `it should apply the age gate set through config set`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`config`, `set`, `npmMinimalAgeGate`, `1d`, WITHOUT_ENV_OVERRIDES);
        await expect(readRc(path)).resolves.toEqual(`npmMinimalAgeGate: 1d\n`);

        await expect(run(`add`, `release-date@1.1.1`, WITHOUT_ENV_OVERRIDES)).rejects.toThrowError(`No candidates found`);

        await run(`config`, `set`, `npmMinimalAgeGate`, `0`, WITHOUT_ENV_OVERRIDES);
        await expect(readRc(path)).resolves.toEqual(`npmMinimalAgeGate: 0s\n`);

        await run(`add`, `release-date@1.1.1`, WITHOUT_ENV_OVERRIDES);

        await expect(source(`require('release-date/package.json').version`, WITHOUT_ENV_OVERRIDES)).resolves.toEqual(`1.1.1`);
      }),
    );

    test(
      `it should apply the age gate of package rules set through config set`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`config`, `set`, `npmMinimalAgeGate`, `0`, WITHOUT_ENV_OVERRIDES);
        await run(`config`, `set`, `packageRules`, `--json`, JSON.stringify([{
          packageFilter: `release-date`,
          npmMinimalAgeGate: `1d`,
        }]), WITHOUT_ENV_OVERRIDES);

        await expect(readRc(path)).resolves.toEqual([
          `npmMinimalAgeGate: 0s\n`,
          `packageRules:\n`,
          `  - packageFilter: release-date\n`,
          `    npmMinimalAgeGate: 1d\n`,
        ].join(``));

        await expect(run(`add`, `release-date@1.1.1`, WITHOUT_ENV_OVERRIDES)).rejects.toThrowError(`No candidates found`);

        await run(`add`, `release-date@^1.0.0`, WITHOUT_ENV_OVERRIDES);
        await expect(source(`require('release-date/package.json').version`, WITHOUT_ENV_OVERRIDES)).resolves.toEqual(`1.1.0`);
      }),
    );

    test(
      `it should quote keys that YAML can't read as plain scalars`,
      makeTemporaryEnv({
        dependencies: {
          [`@scoped/no-deps`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`config`, `set`, `packageExtensions`, `--json`, JSON.stringify({
          [`@scoped/no-deps@*`]: {
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
          },
        }));

        await expect(readRc(path)).resolves.toEqual([
          `packageExtensions:\n`,
          `  "@scoped/no-deps@*":\n`,
          `    dependencies:\n`,
          `      no-deps: 1.0.0\n`,
        ].join(``));

        await run(`install`);

        await expect(source(`require('module').createRequire(require.resolve('@scoped/no-deps/package.json'))('no-deps')`)).resolves.toMatchObject({
          name: `no-deps`,
          version: `1.0.0`,
        });
      }),
    );

    test(
      `it should round-trip awkward string values`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        for (const value of AWKWARD_STRINGS) {
          // Values starting with a dash would be read as options
          await run(`config`, `set`, `initScope`, `--json`, JSON.stringify(value));

          expect(parseSyml(await readRc(path))).toEqual({initScope: value});

          const {stdout} = await run(`config`, `get`, `initScope`);
          expect(stdout).toEqual(`${value}\n`);

          await expect(run(`config`, `get`, `--json`, `initScope`)).resolves.toMatchObject({
            stdout: `${JSON.stringify(value)}\n`,
          });

          // What `config get` prints must be accepted back
          await run(`config`, `set`, `initScope`, stdout.slice(0, -1));
          expect(parseSyml(await readRc(path))).toEqual({initScope: value});
        }
      }),
    );

    test(
      `it should round-trip awkward keys`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        // The path syntax doesn't support escape sequences such as `\n`
        const keys = AWKWARD_STRINGS.filter(key => !key.match(/[\n\t]/));

        for (const key of keys)
          await run(`config`, `set`, `npmScopes[${JSON.stringify(key)}].npmRegistryServer`, `https://example.org/${keys.indexOf(key)}`);

        const parsed = parseSyml(await readRc(path));
        expect(Object.keys(parsed.npmScopes).sort()).toEqual([...keys].sort());

        for (const key of keys) {
          expect(parsed.npmScopes[key]).toEqual({npmRegistryServer: `https://example.org/${keys.indexOf(key)}`});

          await expect(run(`config`, `get`, `npmScopes[${JSON.stringify(key)}].npmRegistryServer`)).resolves.toMatchObject({
            stdout: `https://example.org/${keys.indexOf(key)}\n`,
          });
        }
      }),
    );

    test(
      `it should create the missing entries of map settings`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await expect(run(`config`, `set`, `npmScopes.foo.npmAlwaysAuth`, `true`)).resolves.toMatchObject({
          stdout: expect.stringContaining(`Successfully set npmScopes.foo.npmAlwaysAuth to true`),
        });

        await run(`config`, `set`, `npmRegistries["https://example.org"].npmAuthToken`, `secret`);

        await expect(readRc(path)).resolves.toEqual([
          `npmRegistries:\n`,
          `  "https://example.org":\n`,
          `    npmAuthToken: secret\n`,
          `npmScopes:\n`,
          `  foo:\n`,
          `    npmAlwaysAuth: true\n`,
        ].join(``));

        await expect(run(`config`, `get`, `npmScopes.foo.npmAlwaysAuth`)).resolves.toMatchObject({
          stdout: `true\n`,
        });
      }),
    );

    test(
      `it should create new scope and registry entries that installs then use`,
      makeTemporaryEnv({
        dependencies: {[`@private/package`]: `1.0.0`},
      }, async ({path, run, source}) => {
        const url = await tests.startPackageServer();
        const token = tests.validLogins.fooUser.npmAuthToken!;

        await run(`config`, `set`, `npmScopes.private.npmRegistryServer`, url);
        await run(`config`, `set`, `npmRegistries[${JSON.stringify(`${url}/`)}].npmAuthToken`, token);

        await expect(readRc(path)).resolves.toEqual([
          `npmRegistries:\n`,
          `  "${url}/":\n`,
          `    npmAuthToken: ${token}\n`,
          `npmScopes:\n`,
          `  private:\n`,
          `    npmRegistryServer: "${url}"\n`,
        ].join(``));

        await run(`install`);

        await expect(source(`require('@private/package')`)).resolves.toMatchObject({
          name: `@private/package`,
          version: `1.0.0`,
        });
      }),
    );

    test(
      `it should still reject unknown settings`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await expect(run(`config`, `set`, `notASetting`, `true`)).rejects.toMatchObject({
          stdout: expect.stringContaining(`Configuration key not found (notASetting)`),
        });

        await expect(run(`config`, `set`, `npmScopes.foo.notASetting`, `true`)).rejects.toMatchObject({
          stdout: expect.stringContaining(`Configuration key not found (notASetting)`),
        });

        await expect(xfs.existsPromise(ppath.join(path, Filename.rc))).resolves.toEqual(false);
      }),
    );

    test(
      `it should create missing entries in the home configuration`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        const tmpDir = await xfs.mktempPromise();

        await run(`config`, `set`, `-H`, `npmScopes.foo.npmAlwaysAuth`, `true`, {cwd: tmpDir});
        await run(`config`, `set`, `-H`, `pypiMinimalAgeGate`, `3h`, {cwd: tmpDir});

        await expect(xfs.readFilePromise(ppath.join(path, `..`, Filename.rc), `utf8`)).resolves.toEqual([
          `npmScopes:\n`,
          `  foo:\n`,
          `    npmAlwaysAuth: true\n`,
          `pypiMinimalAgeGate: 3h\n`,
        ].join(``));

        await expect(run(`config`, `get`, `npmScopes.foo.npmAlwaysAuth`)).resolves.toMatchObject({
          stdout: `true\n`,
        });

        await expect(run(`config`, `get`, `pypiMinimalAgeGate`)).resolves.toMatchObject({
          stdout: `3h\n`,
        });
      }),
    );

    test(
      `it should preserve the formatting of the untouched parts of the file`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        const original = [
          `# Settings for the project\n`,
          `\n`,
          `enableTelemetry:    false    # aligned comment\n`,
          `npmScopes:\n`,
          `    # A comment about the scope\n`,
          `    'yarnpkg':\n`,
          `        npmRegistryServer: 'https://npm.example.org'\n`,
          `\n`,
          `packageExtensions:\n`,
          `    "foo@*":\n`,
          `        dependencies: {bar: '*'}\n`,
          `# Trailing comment\n`,
        ].join(``);

        await xfs.writeFilePromise(ppath.join(path, Filename.rc), original);

        await run(`config`, `set`, `npmScopes.@other.npmAlwaysAuth`, `true`);
        await run(`config`, `set`, `npmMinimalAgeGate`, `3h`, WITHOUT_ENV_OVERRIDES);

        await expect(readRc(path)).resolves.toEqual([
          `# Settings for the project\n`,
          `\n`,
          `enableTelemetry:    false    # aligned comment\n`,
          `npmMinimalAgeGate: 3h\n`,
          `npmScopes:\n`,
          `    # A comment about the scope\n`,
          `    "@other":\n`,
          `        npmAlwaysAuth: true\n`,
          `    'yarnpkg':\n`,
          `        npmRegistryServer: 'https://npm.example.org'\n`,
          `\n`,
          `packageExtensions:\n`,
          `    "foo@*":\n`,
          `        dependencies: {bar: '*'}\n`,
          `# Trailing comment\n`,
        ].join(``));

        await run(`config`, `unset`, `npmScopes.@other.npmAlwaysAuth`);
        await run(`config`, `unset`, `npmMinimalAgeGate`);

        await expect(readRc(path)).resolves.toEqual(original);
      }),
    );
  });
});
