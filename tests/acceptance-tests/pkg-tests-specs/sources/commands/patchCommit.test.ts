import {Filename, npath, ppath, xfs} from '@yarnpkg/fslib';

describe(`Commands`, () => {
  describe(`patch-commit`, () => {
    test(
      `it should generate a patch from a package folder`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        const {stdout} = await run(`patch`, `no-deps`, `--json`);
        const {path: updateFolderN} = JSON.parse(stdout);

        const updateFolder = npath.toPortablePath(updateFolderN);
        const updateFile = ppath.join(updateFolder, `index.js`);

        const fileSource = await xfs.readFilePromise(updateFile, `utf8`);
        const fileUser = fileSource.replace(`require(dep)`, `42`);
        await xfs.writeFilePromise(updateFile, fileUser);

        await expect(run(`patch-commit`, updateFolder)).resolves.toMatchSnapshot();
      }),
    );

    test(
      `it should save patches into a local folder if requested`,
      makeTemporaryEnv({
        dependencies: {
          [`one-fixed-dep`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        const {stdout} = await run(`patch`, `one-fixed-dep`, `--json`);
        const {path: updateFolderN} = JSON.parse(stdout);

        const updateFolder = npath.toPortablePath(updateFolderN);
        const updateFile = ppath.join(updateFolder, `index.js`);

        const fileSource = await xfs.readFilePromise(updateFile, `utf8`);
        const fileUser = fileSource.replace(`require(dep)`, `42`);
        await xfs.writeFilePromise(updateFile, fileUser);

        await run(`patch-commit`, `-s`, npath.fromPortablePath(updateFolder));
        await run(`install`);

        await expect(source(`require('one-fixed-dep')`)).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: 42,
          },
        });
      }),
    );

    test(
      `it should only look for the patch metadata in the ancestors of a relative patch folder`,
      makeTemporaryEnv({
        dependencies: {
          [`one-fixed-dep`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        const {stdout} = await run(`patch`, `one-fixed-dep`, `--json`);
        const {path: updateFolderN} = JSON.parse(stdout);

        const updateFolder = npath.toPortablePath(updateFolderN);
        const updateFile = ppath.join(updateFolder, `index.js`);

        const fileSource = await xfs.readFilePromise(updateFile, `utf8`);
        const fileUser = fileSource.replace(`require(dep)`, `42`);
        await xfs.writeFilePromise(updateFile, fileUser);

        // The patch folder lives outside of the project, so its relative path
        // starts with `..`; the current directory isn't one of its ancestors
        // and its content must be ignored.
        const relativeUpdateFolder = ppath.relative(path, updateFolder);
        expect(relativeUpdateFolder.startsWith(`../`)).toBe(true);

        await xfs.writeFilePromise(ppath.join(path, `.locator`), `not a locator`);

        await run(`patch-commit`, `-s`, npath.fromPortablePath(relativeUpdateFolder));
        await run(`install`);

        await expect(source(`require('one-fixed-dep')`)).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: 42,
          },
        });
      }),
    );

    test(
      `it should reference patches from the workspace dependencies when possible`,
      makeTemporaryEnv({
        dependencies: {
          [`one-fixed-dep`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        const {stdout} = await run(`patch`, `one-fixed-dep`, `--json`);
        const {path: updateFolderN} = JSON.parse(stdout);

        const updateFolder = npath.toPortablePath(updateFolderN);
        const updateFile = ppath.join(updateFolder, `index.js`);

        const fileSource = await xfs.readFilePromise(updateFile, `utf8`);
        const fileUser = fileSource.replace(`require(dep)`, `42`);
        await xfs.writeFilePromise(updateFile, fileUser);

        await run(`patch-commit`, `-s`, npath.fromPortablePath(updateFolder));

        const manifest = await xfs.readJsonPromise(ppath.join(path, Filename.manifest));

        expect(manifest.dependencies).toEqual({
          [`one-fixed-dep`]: expect.stringMatching(/^patch:one-fixed-dep/),
        });

        expect(manifest).not.toHaveProperty(`resolutions`);
      }),
    );

    test(
      `it should reference patches using the 'resolutions' field when required`,
      makeTemporaryEnv({
        dependencies: {
          [`one-fixed-dep`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        const {stdout} = await run(`patch`, `no-deps`, `--json`);
        const {path: updateFolderN} = JSON.parse(stdout);

        const updateFolder = npath.toPortablePath(updateFolderN);
        const updateFile = ppath.join(updateFolder, `index.js`);

        const fileSource = await xfs.readFilePromise(updateFile, `utf8`);
        const fileUser = fileSource.replace(`require(dep)`, `42`);
        await xfs.writeFilePromise(updateFile, fileUser);

        await run(`patch-commit`, `-s`, npath.fromPortablePath(updateFolder));

        const manifest = await xfs.readJsonPromise(ppath.join(path, Filename.manifest));

        expect(manifest.dependencies).toEqual({
          [`one-fixed-dep`]: `1.0.0`,
        });

        expect(manifest.resolutions).toEqual({
          [`no-deps@1.0.0`]: expect.stringMatching(/^patch:no-deps/),
        });

        await run(`install`);

        await expect(source(`require('fs').readFileSync(require.resolve('no-deps', {paths: [require.resolve('one-fixed-dep/package.json')]}), 'utf8')`)).resolves.toContain(`= 42;`);
      }),
    );

    test(
      `it should replace the patch of a transitive dependency when calling patch-commit again`,
      makeTemporaryEnv({
        dependencies: {
          [`one-fixed-dep`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        for (const value of [42, 21]) {
          const {stdout} = await run(`patch`, `no-deps`, `--json`);
          const {path: updateFolderN} = JSON.parse(stdout);

          const updateFolder = npath.toPortablePath(updateFolderN);
          const updateFile = ppath.join(updateFolder, `index.js`);

          await xfs.writeFilePromise(updateFile, `module.exports = ${value};\n`);

          await run(`patch-commit`, `-s`, npath.fromPortablePath(updateFolder));

          const manifest = await xfs.readJsonPromise(ppath.join(path, Filename.manifest));

          expect(manifest.resolutions).toEqual({
            [`no-deps@1.0.0`]: expect.stringMatching(/^patch:no-deps%40npm%3A1\.0\.0#/),
          });

          await run(`install`);

          await expect(source(`require('one-fixed-dep')`)).resolves.toMatchObject({
            dependencies: {
              [`no-deps`]: value,
            },
          });
        }
      }),
    );

    test(
      `it should reference patches of dependencies declared with a star range using the 'resolutions' field`,
      makeTemporaryEnv({
        dependencies: {
          [`star-parent`]: `portal:./star-parent`,
        },
      }, async ({path, run, source}) => {
        await xfs.mkdirPromise(ppath.join(path, `star-parent`), {recursive: true});
        await xfs.writeJsonPromise(ppath.join(path, `star-parent/package.json`), {
          name: `star-parent`,
          version: `1.0.0`,
          dependencies: {
            [`no-deps`]: `*`,
          },
        });
        await xfs.writeFilePromise(ppath.join(path, `star-parent/index.js`), `module.exports = require('no-deps');\n`);

        await run(`install`);

        const {stdout} = await run(`patch`, `no-deps`, `--json`);
        const {path: updateFolderN} = JSON.parse(stdout);

        const updateFolder = npath.toPortablePath(updateFolderN);
        await xfs.writeFilePromise(ppath.join(updateFolder, `index.js`), `module.exports = 42;\n`);

        await run(`patch-commit`, `-s`, npath.fromPortablePath(updateFolder));

        const manifest = await xfs.readJsonPromise(ppath.join(path, Filename.manifest));

        expect(manifest.resolutions).toEqual({
          [`no-deps@*`]: expect.stringMatching(/^patch:no-deps/),
        });

        await run(`install`);

        await expect(source(`require('star-parent')`)).resolves.toEqual(42);
      }),
    );

    test(
      `it should honour the patch resolutions written by Yarn 4`,
      makeTemporaryEnv({
        dependencies: {
          [`one-fixed-dep`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        const {stdout} = await run(`patch`, `no-deps`, `--json`);
        const {path: updateFolderN} = JSON.parse(stdout);

        const updateFolder = npath.toPortablePath(updateFolderN);
        await xfs.writeFilePromise(ppath.join(updateFolder, `index.js`), `module.exports = 42;\n`);

        const {stdout: diff} = await run(`patch-commit`, npath.fromPortablePath(updateFolder));

        // This is the layout `yarn patch-commit -s` produces with Yarn 4
        const patchPath = `.yarn/patches/no-deps-npm-1.0.0-8cd9e3ee0b.patch`;
        await xfs.mkdirPromise(ppath.join(path, `.yarn/patches`), {recursive: true});
        await xfs.writeFilePromise(ppath.join(path, patchPath), diff);

        const manifestPath = ppath.join(path, Filename.manifest);
        const manifest = await xfs.readJsonPromise(manifestPath);
        manifest.resolutions = {
          [`no-deps@npm:1.0.0`]: `patch:no-deps@npm%3A1.0.0#~/${patchPath}`,
        };
        await xfs.writeJsonPromise(manifestPath, manifest);

        await run(`install`);

        await expect(source(`require('one-fixed-dep')`)).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: 42,
          },
        });

        // Committing a new version of the patch must update the existing entry rather than add a second one
        {
          const {stdout} = await run(`patch`, `no-deps`, `--json`);
          const {path: updateFolderN} = JSON.parse(stdout);

          const updateFolder = npath.toPortablePath(updateFolderN);
          await xfs.writeFilePromise(ppath.join(updateFolder, `index.js`), `module.exports = 21;\n`);

          await run(`patch-commit`, `-s`, npath.fromPortablePath(updateFolder));
        }

        await expect(xfs.readJsonPromise(manifestPath)).resolves.toMatchObject({
          resolutions: {
            [`no-deps@npm:1.0.0`]: expect.stringMatching(/^patch:no-deps%40npm%3A1\.0\.0#/),
          },
        });

        expect(Object.keys((await xfs.readJsonPromise(manifestPath)).resolutions)).toEqual([`no-deps@npm:1.0.0`]);

        await run(`install`);

        await expect(source(`require('one-fixed-dep')`)).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: 21,
          },
        });
      }),
    );

    test(
      `it should apply patches to files whose names git quotes`,
      makeTemporaryEnv({
        dependencies: {
          [`special-names`]: `file:./special-names`,
        },
      }, async ({path, run, source}) => {
        // Git quotes the names containing non-ASCII characters (unless
        // core.quotePath is disabled), double quotes, backslashes, and control
        // characters. Windows forbids double quotes and control characters in
        // file names, and backslashes are path separators there. It doesn't
        // quote the names containing spaces, but follows them with a tab.
        const names = [`café.js`, `with space.js`];
        if (process.platform !== `win32`)
          names.push(`with "quote".js`, `tab\there.js`, `back\\slash.js`);

        const packagePath = ppath.join(path, `special-names`);
        await xfs.mkdirPromise(packagePath);
        await xfs.writeJsonPromise(ppath.join(packagePath, Filename.manifest), {name: `special-names`, version: `1.0.0`});

        for (const name of names)
          await xfs.writeFilePromise(ppath.join(packagePath, name as Filename), `module.exports = 'original';\n`);

        await run(`install`);

        const {stdout} = await run(`patch`, `special-names`, `--json`);
        const {path: updateFolderN} = JSON.parse(stdout);

        const updateFolder = npath.toPortablePath(updateFolderN);

        for (const name of names)
          await xfs.writeFilePromise(ppath.join(updateFolder, name as Filename), `module.exports = 'patched';\n`);

        await xfs.writeFilePromise(ppath.join(updateFolder, `new café.js`), `module.exports = 'created';\n`);

        await run(`patch-commit`, `-s`, npath.fromPortablePath(updateFolder));

        const patchFolder = ppath.join(path, `.yarn/patches`);
        const patchFiles = await xfs.readdirPromise(patchFolder);
        expect(patchFiles).toHaveLength(1);

        const patch = await xfs.readFilePromise(ppath.join(patchFolder, patchFiles[0]!), `utf8`);

        expect(patch).toContain(`+++ "b/caf\\303\\251.js"`);
        expect(patch).toContain(`+++ "b/new caf\\303\\251.js"\t`);

        await run(`install`);

        for (const name of names)
          await expect(source(`require(${JSON.stringify(`special-names/${name}`)})`)).resolves.toEqual(`patched`);

        await expect(source(`require('special-names/new café.js')`)).resolves.toEqual(`created`);
      }),
    );

    test(
      `it should replace the patch when calling patch-commit again, not wrap it into another patch layer`,
      makeTemporaryEnv({
        dependencies: {
          [`one-fixed-dep`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        {
          const {stdout} = await run(`patch`, `one-fixed-dep`, `--json`);
          const {path: updateFolderN} = JSON.parse(stdout);

          const updateFolder = npath.toPortablePath(updateFolderN);
          const updateFile = ppath.join(updateFolder, `new.js`);

          const fileUser = `module.exports = 42;\n`;
          await xfs.writeFilePromise(updateFile, fileUser);

          await run(`patch-commit`, `-s`, npath.fromPortablePath(updateFolder));

          const manifest = await xfs.readJsonPromise(ppath.join(path, Filename.manifest));

          expect(manifest.dependencies).toEqual({
            [`one-fixed-dep`]: expect.stringMatching(/^patch:one-fixed-dep/),
          });
        }

        {
          const {stdout} = await run(`patch`, `one-fixed-dep`, `--json`);
          const {path: updateFolderN} = JSON.parse(stdout);

          const updateFolder = npath.toPortablePath(updateFolderN);
          const updateFile = ppath.join(updateFolder, `new.js`);

          const fileUser = `module.exports = 21;\n`;
          await xfs.writeFilePromise(updateFile, fileUser);

          await run(`patch-commit`, `-s`, npath.fromPortablePath(updateFolder));

          const manifest = await xfs.readJsonPromise(ppath.join(path, Filename.manifest));

          expect(manifest.dependencies).toEqual({
            [`one-fixed-dep`]: expect.stringMatching(/^patch:one-fixed-dep/),
          });
        }
      }),
    );

    test(
      `it should be able to patch virtual packages`,
      makeTemporaryEnv({
        dependencies: {
          [`peer-deps-lvl0`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        const {stdout} = await run(`patch`, `peer-deps-lvl1`, `--json`);
        const {path: updateFolderN} = JSON.parse(stdout);

        const updateFolder = npath.toPortablePath(updateFolderN);
        const updateFile = ppath.join(updateFolder, `index.js`);

        const fileSource = await xfs.readFilePromise(updateFile, `utf8`);
        const fileUser = fileSource.replace(`require(dep)`, `42`);
        await xfs.writeFilePromise(updateFile, fileUser);

        await run(`patch-commit`, `-s`, npath.fromPortablePath(updateFolder));

        const manifest = await xfs.readJsonPromise(ppath.join(path, Filename.manifest));

        expect(manifest.dependencies).toEqual({
          [`peer-deps-lvl0`]: `1.0.0`,
        });

        expect(manifest.resolutions).toEqual({
          [`peer-deps-lvl1@1.0.0`]: expect.stringMatching(/^patch:peer-deps-lvl1/),
        });

        await run(`install`);
      }),
    );
  });
});
