import {Filename, PortablePath, ppath, xfs} from '@yarnpkg/fslib';

describe(`Features`, () => {
  describe(`Content-Addressed Index`, () => {
    if (process.platform !== `win32`) {
      test(
        `it should preserve executable mode when installing`,
        makeTemporaryEnv({
          dependencies: {
            [`has-bin-entries`]: `1.0.0`,
          },
        }, {
          nodeLinker: `pnpm`,
        }, async ({path, run, source}) => {
          await run(`install`);

          const stat = await xfs.statPromise(ppath.join(path, `node_modules/has-bin-entries/bin-with-exit-code.js`));
          const executableBits = 0o111;
          expect(stat.mode & executableBits).toEqual(executableBits);
        }),
      );
    }

    test(
      `it should use the exact same device/inode for the same file from the same package`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, {
        nodeLinker: `pnpm`,
      }, async ({path, run, source}) => {
        await xfs.mktempPromise(async path2 => {
          await xfs.writeJsonPromise(ppath.join(path2, Filename.manifest), {
            name: `my-package`,
            dependencies: {
              [`no-deps`]: `1.0.0`,
            },
          });

          await run(`install`, {cwd: path});
          await run(`install`, {cwd: path2});

          const statA = await xfs.statPromise(ppath.join(path, `node_modules/no-deps/package.json`));
          const statB = await xfs.statPromise(ppath.join(path2, `node_modules/no-deps/package.json`));

          expect({
            dev: statA.dev,
            ino: statA.ino,
          }).toEqual({
            dev: statB.dev,
            ino: statB.ino,
          });
        });
      }),
    );

    test(
      `it should use the exact same device/inode for the same file from different packages`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, {
        nodeLinker: `pnpm`,
      }, async ({path, run, source}) => {
        await xfs.mktempPromise(async path2 => {
          await xfs.writeJsonPromise(ppath.join(path2, Filename.manifest), {
            name: `my-package`,
            dependencies: {
              [`no-deps`]: `2.0.0`,
            },
          });

          await run(`install`, {cwd: path});
          await run(`install`, {cwd: path2});

          const statA = await xfs.statPromise(ppath.join(path, `node_modules/no-deps/index.js`));
          const statB = await xfs.statPromise(ppath.join(path2, `node_modules/no-deps/index.js`));

          expect({
            dev: statA.dev,
            ino: statA.ino,
          }).toEqual({
            dev: statB.dev,
            ino: statB.ino,
          });
        });
      }),
    );

    test(
      `it should detect when an index file was modified, and automatically repair it`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, {
        nodeLinker: `pnpm`,
      }, async ({path, run, source}) => {
        await run(`install`);

        const referenceFile = ppath.join(path, `node_modules/no-deps/index.js`);

        const originalContent = await xfs.readFilePromise(referenceFile, `utf8`);
        const newContent = `${originalContent}// oh no, modified\n`;

        await xfs.writeFilePromise(referenceFile, newContent);

        // Repairing damage inside node_modules needs --force (the
        // default in interactive terminals).
        await run(`install`, `--force`);

        await expect(xfs.readFilePromise(referenceFile, `utf8`)).resolves.toEqual(originalContent);
      }),
    );

    test(
      `it should repair the index across all projects, not only the current one`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, {
        nodeLinker: `pnpm`,
      }, async ({path, run, source}) => {
        await xfs.mktempPromise(async path2 => {
          await xfs.writeJsonPromise(ppath.join(path2, Filename.manifest), {
            name: `my-package`,
            dependencies: {
              [`no-deps`]: `2.0.0`,
            },
          });

          await run(`install`, {cwd: path});
          await run(`install`, {cwd: path2});

          const referenceFileA = ppath.join(path, `node_modules/no-deps/index.js`);
          const referenceFileB = ppath.join(path2, `node_modules/no-deps/index.js`);

          const originalContent = await xfs.readFilePromise(referenceFileA, `utf8`);
          const newContent = `${originalContent}// oh no, modified\n`;

          await xfs.writeFilePromise(referenceFileA, newContent);

          // Repairing damage inside node_modules needs --force (the
          // default in interactive terminals).
          await run(`install`, `--force`, {cwd: path});

          await expect(xfs.readFilePromise(referenceFileA, `utf8`)).resolves.toEqual(originalContent);
          await expect(xfs.readFilePromise(referenceFileB, `utf8`)).resolves.toEqual(originalContent);
        });
      }),
    );
  });

  describe(`Copy fallback`, () => {
    const configurations = [
      {name: `nodeLinker: pnpm`, config: {nodeLinker: `pnpm`}},
      {name: `nmMode: hardlinks-global`, config: {nodeLinker: `node-modules`, nmMode: `hardlinks-global`}},
      {name: `nmMode: hardlinks-local`, config: {nodeLinker: `node-modules`, nmMode: `hardlinks-local`}},
    ];

    // Two workspaces sharing a version that can't be hoisted, so that
    // hardlinks-local has something to link
    const manifest = {
      workspaces: [`ws1`, `ws2`],
      dependencies: {
        [`no-deps`]: `1.0.0`,
      },
    };

    const setupWorkspaces = async (path: PortablePath) => {
      for (const workspace of [`ws1`, `ws2`] as Array<Filename>) {
        await xfs.mkdirPromise(ppath.join(path, workspace));
        await xfs.writeJsonPromise(ppath.join(path, workspace, Filename.manifest), {
          name: workspace,
          dependencies: {
            [`no-deps`]: `2.0.0`,
          },
        });
      }
    };

    const crossDeviceEnv = {env: {YARN_INJECT_HARDLINK_ERROR: `EXDEV`}};
    const linkLimitEnv = {env: {YARN_INJECT_HARDLINK_ERROR: `EMLINK`}};

    const countWarnings = (stdout: string) => {
      return stdout.split(`\n`).filter(line => line.includes(`copied instead of hard-linked`)).length;
    };

    for (const {name, config} of configurations) {
      test(
        `it should copy files that can't be hardlinked across filesystems, and warn once (${name})`,
        makeTemporaryEnv(manifest, config, async ({path, run}) => {
          await setupWorkspaces(path);

          const {stdout} = await run(`install`, crossDeviceEnv);

          expect(countWarnings(stdout)).toEqual(1);
          expect(stdout).toContain(`are on different filesystems`);
          expect(stdout).toContain(`https://yarnpkg.com/concepts/hardlinks`);

          const referenceFile = ppath.join(path, `ws2/node_modules/no-deps/package.json`);
          await expect(xfs.readJsonPromise(referenceFile)).resolves.toMatchObject({version: `2.0.0`});
          await expect(xfs.statPromise(referenceFile)).resolves.toMatchObject({nlink: 1});
        }),
      );

      test(
        `it should copy files whose content reached the hard link limit (${name})`,
        makeTemporaryEnv(manifest, config, async ({path, run}) => {
          await setupWorkspaces(path);

          const {stdout} = await run(`install`, linkLimitEnv);

          expect(countWarnings(stdout)).toEqual(1);
          expect(stdout).toContain(`maximum number of hard links per file`);
        }),
      );

      test(
        `it shouldn't rewrite copied files on subsequent installs (${name})`,
        makeTemporaryEnv(manifest, config, async ({path, run}) => {
          await setupWorkspaces(path);

          await run(`install`, crossDeviceEnv);

          const referenceFile = ppath.join(path, `ws2/node_modules/no-deps/index.js`);
          const before = await xfs.statPromise(referenceFile);

          const {stdout} = await run(`install`, crossDeviceEnv);
          const after = await xfs.statPromise(referenceFile);

          expect(countWarnings(stdout)).toEqual(0);
          expect(after.ino).toEqual(before.ino);
          expect(after.ctimeMs).toEqual(before.ctimeMs);
        }),
      );

      test(
        `it shouldn't warn when every file got hardlinked (${name})`,
        makeTemporaryEnv(manifest, config, async ({path, run}) => {
          await setupWorkspaces(path);

          const {stdout} = await run(`install`);

          expect(countWarnings(stdout)).toEqual(0);
        }),
      );
    }

    test(
      `it should keep up-to-date copies when relinking with --force`,
      makeTemporaryEnv(manifest, {
        nodeLinker: `node-modules`,
        nmMode: `hardlinks-global`,
      }, async ({path, run}) => {
        await setupWorkspaces(path);

        await run(`install`, crossDeviceEnv);

        const referenceFile = ppath.join(path, `ws2/node_modules/no-deps/index.js`);
        const before = await xfs.statPromise(referenceFile);

        await run(`install`, `--force`, crossDeviceEnv);
        const after = await xfs.statPromise(referenceFile);

        expect(after.ino).toEqual(before.ino);
        expect(after.ctimeMs).toEqual(before.ctimeMs);
      }),
    );

    test(
      `it should hardlink copied files again once linking works`,
      makeTemporaryEnv(manifest, {
        nodeLinker: `node-modules`,
        nmMode: `hardlinks-global`,
      }, async ({path, run}) => {
        await setupWorkspaces(path);

        await run(`install`, crossDeviceEnv);

        const referenceFile = ppath.join(path, `ws2/node_modules/no-deps/package.json`);
        await expect(xfs.statPromise(referenceFile)).resolves.toMatchObject({nlink: 1});

        const {stdout} = await run(`install`, `--force`);

        expect(countWarnings(stdout)).toEqual(0);

        // Both workspaces' copies and the index entry
        await expect(xfs.statPromise(referenceFile)).resolves.toMatchObject({nlink: 3});
      }),
    );

    for (const {name, config} of configurations.slice(0, 2)) {
      test(
        `it should repair modified copies with --force (${name})`,
        makeTemporaryEnv(manifest, config, async ({path, run}) => {
          await setupWorkspaces(path);

          await run(`install`, crossDeviceEnv);

          const referenceFile = ppath.join(path, `ws2/node_modules/no-deps/index.js`);

          const originalContent = await xfs.readFilePromise(referenceFile, `utf8`);
          await xfs.writeFilePromise(referenceFile, `${originalContent}// oh no, modified\n`);

          await run(`install`, `--force`, crossDeviceEnv);

          await expect(xfs.readFilePromise(referenceFile, `utf8`)).resolves.toEqual(originalContent);
        }),
      );
    }

    test(
      `it should let logFilters silence the warning`,
      makeTemporaryEnv(manifest, {
        nodeLinker: `pnpm`,
      }, async ({path, run}) => {
        await setupWorkspaces(path);

        await run(`config`, `set`, `logFilters`, `--json`, JSON.stringify([{
          pattern: `*copied instead of hard-linked*`,
          level: `discard`,
        }]));

        const {stdout} = await run(`install`, crossDeviceEnv);

        expect(countWarnings(stdout)).toEqual(0);
      }),
    );

    if (process.platform !== `win32`) {
      test(
        `it should preserve executable mode when copying`,
        makeTemporaryEnv({
          dependencies: {
            [`has-bin-entries`]: `1.0.0`,
          },
        }, {
          nodeLinker: `pnpm`,
        }, async ({path, run}) => {
          await run(`install`, crossDeviceEnv);

          const stat = await xfs.statPromise(ppath.join(path, `node_modules/has-bin-entries/bin-with-exit-code.js`));
          const executableBits = 0o111;
          expect(stat.mode & executableBits).toEqual(executableBits);
          expect(stat.nlink).toEqual(1);
        }),
      );
    }
  });
});
