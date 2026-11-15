import {Filename, PortablePath, npath, ppath, xfs} from '@yarnpkg/fslib';

describe(`Features`, () => {
  describe(`Configuration`, () => {
    test(`it should let the project configuration override the home configuration`, makeTemporaryEnv({
    }, async ({path, run, source}) => {
      await xfs.mkdirPromise(ppath.join(path, `..`), {recursive: true});

      await xfs.writeJsonPromise(ppath.join(path, `..`, Filename.rc), {
        preferInteractive: true,
      });

      // Sanity check
      await expect(run(`config`, `get`, `--json`, `preferInteractive`)).resolves.toMatchObject({
        stdout: `true\n`,
      });

      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        preferInteractive: false,
      });

      await expect(run(`config`, `get`, `--json`, `preferInteractive`)).resolves.toMatchObject({
        stdout: `false\n`,
      });
    }));

    test(`it should let the env configuration override the project configuration`, makeTemporaryEnv({
    }, async ({path, run, source}) => {
      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        preferInteractive: true,
      });

      // Sanity check
      await expect(run(`config`, `get`, `--json`, `preferInteractive`)).resolves.toMatchObject({
        stdout: `true\n`,
      });

      await expect(run(`config`, `get`, `--json`, `preferInteractive`, {
        env: {YARN_PREFER_INTERACTIVE: `0`},
      })).resolves.toMatchObject({
        stdout: `false\n`,
      });
    }));

    test(`it should skip the home configuration if onConflict: reset is set in the project configuration`, makeTemporaryEnv({
    }, async ({path, run, source}) => {
      await xfs.mkdirPromise(ppath.join(path, `..`), {recursive: true});

      // Sanity checks
      await expect(run(`config`, `get`, `--json`, `preferInteractive`)).resolves.toMatchObject({stdout: `false\n`});
      await expect(run(`config`, `get`, `--json`, `preferTruncatedLines`)).resolves.toMatchObject({stdout: `false\n`});

      await xfs.writeJsonPromise(ppath.join(path, `..`, Filename.rc), {
        preferInteractive: true,
        preferTruncatedLines: true,
      });

      await expect(run(`config`, `get`, `--json`, `preferInteractive`)).resolves.toMatchObject({stdout: `true\n`});
      await expect(run(`config`, `get`, `--json`, `preferTruncatedLines`)).resolves.toMatchObject({stdout: `true\n`});

      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        onConflict: `reset`,
        preferInteractive: true,
      });

      await expect(run(`config`, `get`, `--json`, `preferInteractive`)).resolves.toMatchObject({stdout: `true\n`});
      await expect(run(`config`, `get`, `--json`, `preferTruncatedLines`)).resolves.toMatchObject({stdout: `false\n`});

      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        onConflict: `reset`,
      });

      await expect(run(`config`, `get`, `--json`, `preferInteractive`)).resolves.toMatchObject({stdout: `false\n`});
      await expect(run(`config`, `get`, `--json`, `preferTruncatedLines`)).resolves.toMatchObject({stdout: `false\n`});
    }));

    test(`it should allow extending values from the home configuration if explicitly requested`, makeTemporaryEnv({
    }, async ({path, run, source}) => {
      await xfs.mkdirPromise(ppath.join(path, `..`), {recursive: true});

      // Sanity checks
      await expect(run(`config`, `get`, `--json`, `preferInteractive`)).resolves.toMatchObject({stdout: `false\n`});
      await expect(run(`config`, `get`, `--json`, `immutablePatterns`)).resolves.toMatchObject({stdout: `[]\n`});

      await xfs.writeJsonPromise(ppath.join(path, `..`, Filename.rc), {
        preferInteractive: true,
        immutablePatterns: [`foo`],
      });

      await expect(run(`config`, `get`, `--json`, `preferInteractive`)).resolves.toMatchObject({stdout: `true\n`});
      await expect(run(`config`, `get`, `--json`, `immutablePatterns`)).resolves.toMatchObject({stdout: `["foo"]\n`});

      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        onConflict: `reset`,
        immutablePatterns: [`bar`],
      });

      await expect(run(`config`, `get`, `--json`, `preferInteractive`)).resolves.toMatchObject({stdout: `false\n`});
      await expect(run(`config`, `get`, `--json`, `immutablePatterns`)).resolves.toMatchObject({stdout: `["bar"]\n`});

      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        onConflict: `reset`,
        immutablePatterns: {
          onConflict: `extend`,
          value: [`bar`],
        },
      });

      await expect(run(`config`, `get`, `--json`, `preferInteractive`)).resolves.toMatchObject({stdout: `false\n`});
      await expect(run(`config`, `get`, `--json`, `immutablePatterns`)).resolves.toMatchObject({stdout: `["foo","bar"]\n`});
    }));

    test(`it should allow resetting a home configuration field without providing a project value`, makeTemporaryEnv({
    }, async ({path, run, source}) => {
      await xfs.mkdirPromise(ppath.join(path, `..`), {recursive: true});

      await xfs.writeJsonPromise(ppath.join(path, `..`, Filename.rc), {
        immutablePatterns: [`foo`],
      });

      await expect(run(`config`, `get`, `--json`, `immutablePatterns`)).resolves.toMatchObject({stdout: `["foo"]\n`});

      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        immutablePatterns: {
          onConflict: `reset`,
        },
      });

      await expect(run(`config`, `get`, `--json`, `immutablePatterns`)).resolves.toMatchObject({stdout: `[]\n`});
    }));

    test(`it should return a helpful error if the rc file is wrong`, makeTemporaryEnv({
    }, async ({path, run, source}) => {
      await xfs.mkdirPromise(ppath.join(path, `..` as PortablePath), {recursive: true});

      await xfs.writeJsonPromise(ppath.join(path, `..` as PortablePath, Filename.rc), {
        packageExtensions: {
          "@lezer/html@*": {
            dependencies: {
              "@lezer/common": `*`,
              // Wrong indentation here, will throw an error:
              "@lezer/javascript@*": {
                dependencies: {
                  "@lezer/common": `*`,
                },
              },
            },
          },
        },
      });

      await expect(run(`config`, `get`, `--json`, `packageExtensions`)).rejects.toMatchObject({stdout: expect.stringContaining(`Invalid ident: @lezer/javascript@*`)});
    }));

    test(`it should report the project rc file and the setting at fault`, makeTemporaryEnv({
    }, async ({path, run, source}) => {
      await xfs.writeFilePromise(ppath.join(path, Filename.rc), [
        `enableTelemetry: false`,
        `npmScopes:`,
        `  foo:`,
        `    npmAlwaysAuth: 12`,
      ].join(`\n`));

      const rcPath = npath.fromPortablePath(ppath.join(path, Filename.rc));

      await expect(run(`config`, `get`, `enableScripts`)).rejects.toMatchObject({
        stdout: expect.stringContaining(`An error occurred while parsing your configuration: ${rcPath}:4:20: npmScopes.foo.npmAlwaysAuth: Invalid value: 12`),
      });
    }));

    test(`it should report the home rc file when it is the one at fault`, makeTemporaryEnv({
    }, async ({path, run, source}) => {
      await xfs.mkdirPromise(ppath.join(path, `..`), {recursive: true});

      await xfs.writeFilePromise(ppath.join(path, `..`, Filename.rc), `nodeLinker: foo\n`);
      await xfs.writeFilePromise(ppath.join(path, Filename.rc), `nodeLinker: pnp\n`);

      const rcPath = npath.fromPortablePath(ppath.join(path, `..`, Filename.rc));

      await expect(run(`config`, `get`, `nodeLinker`)).rejects.toMatchObject({
        stdout: expect.stringContaining(`An error occurred while parsing your configuration: ${rcPath}:1:13: nodeLinker: Invalid enum value (foo)`),
      });
    }));

    test(`it should report the rc file containing malformed yaml`, makeTemporaryEnv({
    }, async ({path, run, source}) => {
      await xfs.mkdirPromise(ppath.join(path, `..`), {recursive: true});

      await xfs.writeFilePromise(ppath.join(path, `..`, Filename.rc), [
        `npmScopes:`,
        `  @foo:`,
        `    npmAlwaysAuth: true`,
      ].join(`\n`));

      const rcPath = npath.fromPortablePath(ppath.join(path, `..`, Filename.rc));

      await expect(run(`config`, `get`, `enableScripts`)).rejects.toMatchObject({
        stdout: expect.stringContaining(`An error occurred while parsing your configuration: ${rcPath}: found character that cannot start any token at line 2 column 3`),
      });
    }));

    test(`it should report the environment variable at fault`, makeTemporaryEnv({
    }, async ({path, run, source}) => {
      await expect(run(`config`, `get`, `enableScripts`, {
        env: {YARN_ENABLE_SCRIPTS: `maybe`},
      })).rejects.toMatchObject({
        stdout: expect.stringContaining(`An error occurred while parsing your configuration: environment variable YARN_ENABLE_SCRIPTS: enableScripts: Invalid value: maybe`),
      });
    }));
  });
});
