const {xfs, npath} = require(`@yarnpkg/fslib`);
const {
  fs: {writeFile},
  misc: {parseJsonStream},
} = require(`pkg-tests-core`);

describe(`Features`, () => {
  describe(`Rc files`, () => {
    test(
      `it should change the configuration when found in the current directory`,
      makeTemporaryEnv(
        {},
        async ({path, run, source}) => {
          await writeFile(`${path}/.yarnrc.yml`, `pnpShebang: "Hello World!"\n`);

          expect(parseJsonStream(
            (await run(`config`, `--json`)).stdout,
            `key`,
          )).toMatchObject({
            [`pnpShebang`]: {
              effective: `Hello World!`,
            },
          });
        },
      ),
    );

    test(
      `it should support changing the rc filename via the environment`,
      makeTemporaryEnv(
        {},
        async ({path, run, source}) => {
          await writeFile(`${path}/.foobarrc`, `pnpShebang: "Hello World!"\n`);

          expect(parseJsonStream(
            (await run(`config`, `--json`, {rcFilename: `.foobarrc`})).stdout,
            `key`,
          )).toMatchObject({
            [`pnpShebang`]: {
              effective: `Hello World!`,
            },
          });
        },
      ),
    );

    test(
      `it should always load home config in loose mode`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await xfs.writeFilePromise(
          `${path}/.yarnrc.yml`,
          `unknownConfigKey: 42\n`,
        );
        await xfs.mkdirPromise(`${path}/project`);
        await xfs.writeJsonPromise(`${path}/project/package.json`, {});
        await xfs.writeFilePromise(`${path}/project/yarn.lock`, ``);

        await expect(
          run(`install`, {
            cwd: `${path}/project`,
            env: {
              HOME: npath.fromPortablePath(path),
              USERPROFILE: npath.fromPortablePath(path),
            },
          }),
        ).resolves.toMatchObject({code: 0});
      }),
    );

    // Yarn prints the native form of paths but keeps forward slashes as separators,
    // including on Windows (`C:/Users/...`)
    const toPrintedPath = p => {
      return npath.fromPortablePath(p).replace(/\\/g, `/`);
    };

    const missingSettingTests = [
      [`an island without a linker`, `unstableIslands:\n  x: {}\n`, `unstableIslands.x.linker`],
      [`an island with other fields but no linker`, `unstableIslands:\n  x:\n    workspaces: [foo]\n`, `unstableIslands.x.linker`],
      [`a log filter without a level`, `logFilters:\n  - text: foo\n    level: info\n  - text: bar\n`, `logFilters[1].level`],
    ];

    for (const [description, rc, setting] of missingSettingTests) {
      test(
        `it should report the missing required settings (${description})`,
        makeTemporaryEnv({}, async ({path, run, source}) => {
          await xfs.writeFilePromise(`${path}/.yarnrc.yml`, rc);

          await expect(run(`install`)).rejects.toMatchObject({
            stdout: expect.stringContaining(`${toPrintedPath(`${path}/.yarnrc.yml`)}: ${setting}: Missing required setting\n`),
          });
        }),
      );
    }

    test(
      `it should report the missing required settings from the home configuration`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await xfs.writeFilePromise(`${path}/.yarnrc.yml`, `unstableIslands:\n  x: {}\n`);
        await xfs.mkdirPromise(`${path}/project`);
        await xfs.writeJsonPromise(`${path}/project/package.json`, {});
        await xfs.writeFilePromise(`${path}/project/yarn.lock`, ``);

        await expect(
          run(`install`, {
            cwd: `${path}/project`,
            env: {
              HOME: npath.fromPortablePath(path),
              USERPROFILE: npath.fromPortablePath(path),
            },
          }),
        ).rejects.toMatchObject({
          stdout: expect.stringContaining(`${toPrintedPath(`${path}/.yarnrc.yml`)}: unstableIslands.x.linker: Missing required setting\n`),
        });
      }),
    );

    test(
      `it shouldn't let config set write an incomplete setting`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await expect(run(`config`, `set`, `--json`, `unstableIslands`, `{"x": {}}`)).rejects.toMatchObject({
          stdout: expect.stringContaining(`Invalid configuration: unstableIslands.x.linker: Missing required setting\n`),
        });

        await expect(xfs.existsPromise(`${path}/.yarnrc.yml`)).resolves.toEqual(false);
      }),
    );
  });
});
