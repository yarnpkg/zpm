import {npath, xfs} from '@yarnpkg/fslib';
import {tests}      from 'pkg-tests-core';

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

    const folderSettings = [
      `cacheFolder`,
      `deferredVersionFolder`,
      `globalFolder`,
      `patchFolder`,
      `pnpUnpluggedFolder`,
      `virtualFolder`,
    ];

    for (const setting of folderSettings) {
      test(
        `it should resolve relative paths from the environment against the project root (${setting})`,
        makeTemporaryEnv({}, async ({path, run, source}) => {
          await xfs.mkdirPromise(`${path}/subfolder`);

          const absolute = await run(`config`, `get`, `--json`, setting, {
            [setting]: npath.fromPortablePath(`${path}/relative/folder`),
          });

          await expect(run(`config`, `get`, `--json`, setting, {
            [setting]: `relative/folder`,
            cwd: `${path}/subfolder`,
          })).resolves.toMatchObject({
            stdout: absolute.stdout,
          });
        }),
      );

      test(
        `it should resolve relative paths from the rc file against the folder of the rc file (${setting})`,
        makeTemporaryEnv({}, async ({path, run, source}) => {
          await xfs.writeFilePromise(`${path}/.yarnrc.yml`, `${setting}: ${JSON.stringify(npath.fromPortablePath(`${path}/relative/folder`))}\n`);
          const absolute = await run(`config`, `get`, `--json`, setting);

          await xfs.writeFilePromise(`${path}/.yarnrc.yml`, `${setting}: ./relative/folder\n`);

          await expect(run(`config`, `get`, `--json`, setting)).resolves.toMatchObject({
            stdout: absolute.stdout,
          });
        }),
      );
    }

    // The files are read when Yarn starts, so they must exist and be valid
    const writeHttpsFiles = async path => {
      const certs = await tests.getHttpsCertificates();

      await xfs.mkdirPromise(`${path}/relative`, {recursive: true});
      await xfs.writeFilePromise(`${path}/relative/ca.crt`, certs.ca.certificate);
      await xfs.writeFilePromise(`${path}/relative/client.crt`, certs.server.certificate);
      await xfs.writeFilePromise(`${path}/relative/client.key`, certs.server.clientKey);
    };

    const httpsSettings = {
      httpsCaFilePath: `relative/ca.crt`,
      httpsCertFilePath: `relative/client.crt`,
      httpsKeyFilePath: `relative/client.key`,
    };

    const getHttpsSettings = async (run, config) => {
      const values = {};

      for (const setting of Object.keys(httpsSettings))
        values[setting] = (await run(`config`, `get`, `--json`, setting, config)).stdout;

      return values;
    };

    test(
      `it should resolve relative paths from the environment against the project root (https settings)`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await writeHttpsFiles(path);

        const absolute = await getHttpsSettings(run, Object.fromEntries(Object.entries(httpsSettings).map(([setting, value]) => {
          return [setting, npath.fromPortablePath(`${path}/${value}`)];
        })));

        await expect(getHttpsSettings(run, httpsSettings)).resolves.toEqual(absolute);
      }),
    );

    test(
      `it should resolve relative paths from the rc file against the folder of the rc file (https settings)`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await writeHttpsFiles(path);

        await xfs.writeFilePromise(`${path}/.yarnrc.yml`, Object.entries(httpsSettings).map(([setting, value]) => {
          return `${setting}: ${JSON.stringify(npath.fromPortablePath(`${path}/${value}`))}\n`;
        }).join(``));

        const absolute = await getHttpsSettings(run, {});

        await xfs.writeFilePromise(`${path}/.yarnrc.yml`, Object.entries(httpsSettings).map(([setting, value]) => {
          return `${setting}: ./${value}\n`;
        }).join(``));

        await expect(getHttpsSettings(run, {})).resolves.toEqual(absolute);
      }),
    );

    test(
      `it should resolve relative paths from the rc file against the folder of the rc file (networkSettings)`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await writeHttpsFiles(path);

        await xfs.writeFilePromise(`${path}/.yarnrc.yml`, `networkSettings:\n  "example.org":\n    httpsCaFilePath: ${JSON.stringify(npath.fromPortablePath(`${path}/relative/ca.crt`))}\n`);
        const absolute = await run(`config`, `get`, `--json`, `networkSettings`);

        await xfs.writeFilePromise(`${path}/.yarnrc.yml`, `networkSettings:\n  "example.org":\n    httpsCaFilePath: ./relative/ca.crt\n`);

        await expect(run(`config`, `get`, `--json`, `networkSettings`)).resolves.toMatchObject({
          stdout: absolute.stdout,
        });
      }),
    );
  });
});
