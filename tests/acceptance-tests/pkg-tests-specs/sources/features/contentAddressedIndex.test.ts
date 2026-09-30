import {Filename, ppath, xfs} from '@yarnpkg/fslib';
import {fs as fsUtils}        from 'pkg-tests-core';

describe(`Features`, () => {
  describe(`Content-Addressed Index`, () => {
    if (process.platform !== `win32`) {
      it(`should install and repair read-only files without changing their permissions`,
        makeTemporaryEnv({
          dependencies: {readonly: `file:./readonly.tgz`},
        }, {nodeLinker: `pnpm`}, async ({path, run}) => {
          const fixture = ppath.join(path, `fixture`);
          await xfs.mkdirPromise(fixture);
          await xfs.writeJsonPromise(ppath.join(fixture, Filename.manifest), {name: `readonly`, version: `1.0.0`});
          await xfs.writeFilePromise(ppath.join(fixture, `index.js`), `original`);
          await xfs.chmodPromise(ppath.join(fixture, `index.js`), 0o555);
          await fsUtils.packToFile(ppath.join(path, `readonly.tgz`), fixture, {virtualPath: ppath.resolve(`/package`)});

          await run(`install`);
          const installed = ppath.join(path, `node_modules/readonly/index.js`);
          expect((await xfs.statPromise(installed)).mode & 0o777).toEqual(0o555);

          // A failed install or external modification can leave a read-only entry needing repair.
          await xfs.chmodPromise(installed, 0o755);
          await xfs.writeFilePromise(installed, `modified`);
          await xfs.chmodPromise(installed, 0o555);
          await run(`install`, `--force`);

          await expect(xfs.readFilePromise(installed, `utf8`)).resolves.toEqual(`original`);
          expect((await xfs.statPromise(installed)).mode & 0o777).toEqual(0o555);
        }),
      );

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
});
