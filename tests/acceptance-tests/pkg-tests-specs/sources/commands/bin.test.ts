import {npath, ppath, xfs, Filename} from '@yarnpkg/fslib';

const configs = [{
  nodeLinker: `pnp`,
}, {
  nodeLinker: `pnpm`,
}, {
  nodeLinker: `node-modules`,
}];

const HAS_BIN_ENTRIES_BINARIES = [
  `has-bin-entries`,
  `has-bin-entries-get-pwd`,
  `has-bin-entries-with-exit-code`,
  `has-bin-entries-with-relative-require`,
  `has-bin-entries-with-require`,
];

function parseNdjson(stdout: string) {
  return stdout.trim().split(`\n`).map(line => JSON.parse(line));
}

async function setupSelfBin(path: string) {
  await xfs.writeFilePromise(ppath.join(npath.toPortablePath(path), `self-bin.js`), `console.log(42);\n`);
}

describe(`Commands`, () => {
  describe(`bin`, () => {
    test(
      `it should report an error when the binary doesn't exist`,
      makeTemporaryEnv({
        name: `my-package`,
      }, async ({path, run, source}) => {
        await run(`install`);

        await expect(run(`bin`, `does-not-exist`)).rejects.toMatchObject({
          code: 1,
          stdout: expect.stringContaining(`Usage Error: Couldn't find a binary named "does-not-exist" for package "my-package@workspace:my-package`),
        });
      }),
    );

    test(
      `it should install the project if needed before listing the binaries`,
      makeTemporaryEnv({
        name: `my-package`,
        dependencies: {
          [`has-bin-entries`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await expect(run(`bin`)).resolves.toMatchObject({
          code: 0,
          stdout: HAS_BIN_ENTRIES_BINARIES.map(name => `${name}\n`).join(``),
        });
      }),
    );

    test(
      `it should list the binaries of the current workspace in a monorepo, not those of the root`,
      makeTemporaryEnv({
        name: `root`,
        private: true,
        workspaces: [`packages/*`],
        dependencies: {
          [`no-deps-bins`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await xfs.mkdirpPromise(ppath.join(path, `packages/workspace-a`));
        await xfs.writeJsonPromise(ppath.join(path, `packages/workspace-a`, Filename.manifest), {
          name: `workspace-a`,
          bin: {
            [`workspace-a-bin`]: `./self-bin.js`,
          },
          dependencies: {
            [`has-bin-entries`]: `1.0.0`,
          },
        });

        await setupSelfBin(npath.fromPortablePath(ppath.join(path, `packages/workspace-a`)));

        await run(`install`);

        await expect(run(`bin`, {cwd: ppath.join(path, `packages/workspace-a`)})).resolves.toMatchObject({
          code: 0,
          stdout: [...HAS_BIN_ENTRIES_BINARIES, `workspace-a-bin`].sort().map(name => `${name}\n`).join(``),
        });

        await expect(run(`bin`)).resolves.toMatchObject({
          code: 0,
          stdout: `no-deps-bins\n`,
        });
      }),
    );

    for (const config of configs) {
      describe(`w/ the ${config.nodeLinker} linker`, () => {
        test(
          `it should list the binaries of the direct dependencies and of the workspace itself, sorted by name`,
          makeTemporaryEnv({
            name: `my-package`,
            bin: {
              [`my-package-bin`]: `./self-bin.js`,
            },
            dependencies: {
              [`has-bin-entries`]: `1.0.0`,
              [`one-fixed-dep-bins`]: `1.0.0`,
            },
          }, config, async ({path, run, source}) => {
            await setupSelfBin(npath.fromPortablePath(path));
            await run(`install`);

            // no-deps-bins is a transitive dependency (through one-fixed-dep-bins), so it isn't listed
            await expect(run(`bin`)).resolves.toMatchObject({
              code: 0,
              stdout: [...HAS_BIN_ENTRIES_BINARIES, `my-package-bin`].sort().map(name => `${name}\n`).join(``),
            });
          }),
        );

        test(
          `it should print nothing when no binary is available`,
          makeTemporaryEnv({
            name: `my-package`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
          }, config, async ({path, run, source}) => {
            await run(`install`);

            await expect(run(`bin`)).resolves.toMatchObject({
              code: 0,
              stdout: ``,
            });
          }),
        );

        test(
          `it should print the providing package of each binary with -v`,
          makeTemporaryEnv({
            name: `my-package`,
            bin: {
              [`my-package-bin`]: `./self-bin.js`,
            },
            dependencies: {
              [`has-bin-entries`]: `1.0.0`,
            },
          }, config, async ({path, run, source}) => {
            await setupSelfBin(npath.fromPortablePath(path));
            await run(`install`);

            const names = [...HAS_BIN_ENTRIES_BINARIES, `my-package-bin`].sort();
            const width = Math.max(...names.map(name => name.length));

            const expected = names.map(name => {
              const locator = name === `my-package-bin`
                ? `my-package@workspace:my-package`
                : `has-bin-entries@npm:1.0.0`;

              return `${name.padEnd(width)}   ${locator}\n`;
            }).join(``);

            await expect(run(`bin`, `-v`)).resolves.toMatchObject({
              code: 0,
              stdout: expected,
            });
          }),
        );

        test(
          `it should print an NDJSON stream with --json, with the same paths as the single-binary form`,
          makeTemporaryEnv({
            name: `my-package`,
            bin: {
              [`my-package-bin`]: `./self-bin.js`,
            },
            dependencies: {
              [`has-bin-entries`]: `1.0.0`,
              [`one-fixed-dep-bins`]: `1.0.0`,
            },
          }, config, async ({path, run, source}) => {
            await setupSelfBin(npath.fromPortablePath(path));
            await run(`install`);

            const {stdout} = await run(`bin`, `--json`);
            const entries = parseNdjson(stdout);

            const names = [...HAS_BIN_ENTRIES_BINARIES, `my-package-bin`].sort();
            expect(entries.map(entry => entry.name)).toEqual(names);

            for (const entry of entries) {
              expect(Object.keys(entry)).toEqual([`name`, `source`, `path`]);
              expect(entry.source).toEqual(entry.name === `my-package-bin` ? `my-package` : `has-bin-entries`);

              const {stdout: singlePath} = await run(`bin`, entry.name);
              expect(entry.path).toEqual(singlePath.trim());

              await expect(
                source(`require('fs').existsSync(${JSON.stringify(entry.path)})`),
              ).resolves.toEqual(true);
            }

            expect(entries.find(entry => entry.name === `my-package-bin`).path).toEqual(
              npath.join(npath.fromPortablePath(path), `self-bin.js`),
            );
          }),
        );

        test(
          `it should report the path to a workspace's own binary`,
          makeTemporaryEnv({
            name: `my-package`,
            bin: {
              [`my-package-bin`]: `./self-bin.js`,
            },
          }, config, async ({path, run, source}) => {
            await setupSelfBin(npath.fromPortablePath(path));
            await run(`install`);

            await expect(run(`bin`, `my-package-bin`)).resolves.toMatchObject({
              code: 0,
              stdout: `${npath.join(npath.fromPortablePath(path), `self-bin.js`)}\n`,
            });
          }),
        );

        test(
          `it should not report the path to a transitive dependency's binary`,
          makeTemporaryEnv({
            name: `my-package`,
            dependencies: {
              [`one-fixed-dep-bins`]: `1.0.0`,
            },
          }, config, async ({path, run, source}) => {
            await run(`install`);

            await expect(run(`bin`, `no-deps-bins`)).rejects.toMatchObject({
              code: 1,
              stdout: expect.stringContaining(`Couldn't find a binary named "no-deps-bins" for package "my-package@workspace:my-package`),
            });
          }),
        );
      });
    }
  });
});
