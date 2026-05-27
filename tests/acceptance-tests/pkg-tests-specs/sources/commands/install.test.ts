import {Filename, xfs, ppath, npath} from '@yarnpkg/fslib';
import {tests, misc}                 from 'pkg-tests-core';

describe(`Commands`, () => {
  describe(`install`, () => {
    test(
      `it should print regular messages as JSON items when using --json`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        const {stdout} = await run(`install`, `--json`);

        expect(misc.parseJsonStream(stdout)).toEqual([{
          data: expect.stringMatching(/^Yarn \d+\.\d+\.\d+/),
          displayName: null,
          indent: `· `,
          name: null,
          type: `info`,
        }, {
          data: `┌ Installing packages`,
          displayName: null,
          indent: ``,
          name: null,
          type: `info`,
        }, {
          data: `└ Completed`,
          displayName: null,
          indent: ``,
          name: null,
          type: `info`,
        }, {
          data: `┌ Linking the project`,
          displayName: null,
          indent: ``,
          name: null,
          type: `info`,
        }, {
          data: `└ Completed`,
          displayName: null,
          indent: ``,
          name: null,
          type: `info`,
        }]);
      }),
    );

    test(
      `it should print the logs to the standard output when using --inline-builds`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps-scripted`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        const {stdout} = await run(`install`, `--inline-builds`);

        expect(stdout).toContain(`no-deps-scripted@npm:1.0.0 must be built because it never has been before`);
        expect(stdout).toContain(`preinstall out`);
      }),
    );

    test(
      `it should skip build scripts when using --mode=skip-build`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps-scripted`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        const {stdout} = await run(`install`, `--inline-builds`, `--mode=skip-build`);

        expect(stdout).not.toContain(`no-deps-scripted@npm:1.0.0 must be built because it never has been before`);
        expect(stdout).not.toContain(`STDOUT preinstall out`);
      }),
    );

    test(
      `it shouldn't impact how artifacts are generated when using --mode=skip-build`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps-scripted`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        const pnpPath = ppath.join(path, Filename.pnpCjs);

        await run(`install`);
        const pnpFileWithBuilds = await xfs.readFilePromise(pnpPath);

        await xfs.removePromise(pnpPath);

        await run(`install`, `--mode=skip-build`);
        const pnpFileWithoutBuilds = await xfs.readFilePromise(pnpPath);

        expect(pnpFileWithBuilds).toEqual(pnpFileWithoutBuilds);
      }),
    );

    tests.testIf(
      () => process.platform !== `win32`,
      `it should install from zips that are symlinks`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        const allFiles = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));
        const zipFiles = allFiles.filter(file => file.endsWith(`.zip`));

        await xfs.mkdirPromise(ppath.join(path, `store`));
        for (const filename of zipFiles) {
          const zipFile = ppath.join(path, `.yarn/cache`, filename);
          const storePath = ppath.join(path, `store`, filename);
          await xfs.movePromise(zipFile, storePath);
          await xfs.symlinkPromise(storePath, zipFile);
        }

        await xfs.removePromise(ppath.join(path, Filename.pnpCjs));

        await run(`install`, `--immutable`);
      }),
    );

    test(
      `it should refuse to create a lockfile when using --immutable`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await expect(run(`install`, `--immutable`)).rejects.toThrow(/The lockfile would have been created by this install/);
      }),
    );

    test(
      `it should refuse to change the lockfile when using --immutable`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`install`);

        await xfs.writeJsonPromise(ppath.join(path, `yarn.lock`), {
          dependencies: {
            [`no-deps`]: `1.0.0`,
          },
        });

        await expect(run(`install`, `--immutable`)).rejects.toThrow(/The lockfile would have been created by this install/);
      }),
    );

    test(
      `it should update the lockfile when using --refresh-lockfile`,
      makeTemporaryEnv({
        dependencies: {
          [`one-fixed-dep`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        // Sanity check
        await expect(source(`require('one-fixed-dep')`)).resolves.toMatchObject({
          name: `one-fixed-dep`,
          version: `1.0.0`,
          dependencies: {
            [`no-deps`]: {
              name: `no-deps`,
              version: `1.0.0`,
            },
          },
        });

        const lockfilePath = ppath.join(path, Filename.lockfile);
        const lockfileContent = await xfs.readFilePromise(lockfilePath, `utf8`);
        const modifiedLockfile = lockfileContent.replace(/"no-deps": "1\.0\.0"/, `"no-deps": "2.0.0"`);
        await xfs.writeFilePromise(lockfilePath, modifiedLockfile);

        await run(`install`);

        // Sanity check
        await expect(source(`require('one-fixed-dep')`)).resolves.toMatchObject({
          name: `one-fixed-dep`,
          version: `1.0.0`,
          dependencies: {
            [`no-deps`]: {
              name: `no-deps`,
              version: `2.0.0`,
            },
          },
        });

        await run(`install`, `--refresh-lockfile`);

        // Actual test
        await expect(source(`require('one-fixed-dep')`)).resolves.toMatchObject({
          name: `one-fixed-dep`,
          version: `1.0.0`,
          dependencies: {
            [`no-deps`]: {
              name: `no-deps`,
              version: `1.0.0`,
            },
          },
        });
      }),
    );

    test(
      `it should block invalid lockfiles when using --refresh-lockfile with --immutable`,
      makeTemporaryEnv({
        dependencies: {
          [`one-fixed-dep`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        const lockfilePath = ppath.join(path, Filename.lockfile);
        const lockfileContent = await xfs.readFilePromise(lockfilePath, `utf8`);
        const modifiedLockfile = lockfileContent.replace(/"no-deps": "1\.0\.0"/, `"no-deps": "2.0.0"`);
        await xfs.writeFilePromise(lockfilePath, modifiedLockfile);

        await run(`install`);

        await expect(run(`install`, `--immutable`, `--refresh-lockfile`)).rejects.toThrow(/The lockfile would have been created by this install/);
      }),
    );

    test(
      `it should enable --refresh-lockfile --immutable by default in public PR CIs`,
      makeTemporaryEnv({
        dependencies: {
          [`one-fixed-dep`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        const lockfilePath = ppath.join(path, Filename.lockfile);
        const lockfileContent = await xfs.readFilePromise(lockfilePath, `utf8`);
        const modifiedLockfile = lockfileContent.replace(/"no-deps": "1\.0\.0"/, `"no-deps": "2.0.0"`);
        await xfs.writeFilePromise(lockfilePath, modifiedLockfile);

        const eventPath = ppath.join(path, `github-event-file.json`);
        await xfs.writeJsonPromise(eventPath, {
          repository: {
            private: false,
          },
        });

        await run(`install`);

        await expect(run(`install`, {
          env: {
            GITHUB_ACTIONS: `true`,
            GITHUB_EVENT_NAME: `pull_request`,
            GITHUB_EVENT_PATH: npath.fromPortablePath(eventPath),
          },
        })).rejects.toThrow(/The lockfile would have been created by this install/);
      }),
    );

    test(
      `it should not enable --refresh-lockfile --immutable in private PR CIs`,
      makeTemporaryEnv({
        dependencies: {
          [`one-fixed-dep`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        const lockfilePath = ppath.join(path, Filename.lockfile);
        const lockfileContent = await xfs.readFilePromise(lockfilePath, `utf8`);
        const modifiedLockfile = lockfileContent.replace(/"no-deps": "1\.0\.0"/, `"no-deps": "2.0.0"`);
        await xfs.writeFilePromise(lockfilePath, modifiedLockfile);

        const eventPath = ppath.join(path, `github-event-file.json`);
        await xfs.writeJsonPromise(eventPath, {
          repository: {
            private: true,
          },
        });

        await run(`install`);

        await run(`install`, {
          env: {
            GITHUB_ACTIONS: `true`,
            GITHUB_EVENT_NAME: `pull_request`,
            GITHUB_EVENT_PATH: npath.fromPortablePath(eventPath),
          },
        });
      }),
    );

    test(
      `it should not enable --refresh-lockfile --immutable if the GH environment file is missing`,
      makeTemporaryEnv({
        dependencies: {
          [`one-fixed-dep`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        const lockfilePath = ppath.join(path, Filename.lockfile);
        const lockfileContent = await xfs.readFilePromise(lockfilePath, `utf8`);
        const modifiedLockfile = lockfileContent.replace(/"no-deps": "1\.0\.0"/, `"no-deps": "2.0.0"`);
        await xfs.writeFilePromise(lockfilePath, modifiedLockfile);

        const eventPath = ppath.join(path, `github-event-file.json`);

        await run(`install`);

        await run(`install`, {
          env: {
            GITHUB_ACTIONS: `true`,
            GITHUB_EVENT_NAME: `pull_request`,
            GITHUB_EVENT_PATH: npath.fromPortablePath(eventPath),
          },
        });
      }),
    );

    test(
      `it should let --immutable=false opt out of the public-PR-CI auto-flip`,
      makeTemporaryEnv({
        dependencies: {
          [`one-fixed-dep`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        const lockfilePath = ppath.join(path, Filename.lockfile);
        const lockfileContent = await xfs.readFilePromise(lockfilePath, `utf8`);
        const modifiedLockfile = lockfileContent.replace(/"no-deps": "1\.0\.0"/, `"no-deps": "2.0.0"`);
        await xfs.writeFilePromise(lockfilePath, modifiedLockfile);

        const eventPath = ppath.join(path, `github-event-file.json`);
        await xfs.writeJsonPromise(eventPath, {
          repository: {
            private: false,
          },
        });

        // Without --no-immutable this would auto-flip to immutable
        // and fail. The explicit opt-out should bypass that,
        // even under the public-PR-CI heuristic.
        await run(`install`, `--no-immutable`, {
          env: {
            GITHUB_ACTIONS: `true`,
            GITHUB_EVENT_NAME: `pull_request`,
            GITHUB_EVENT_PATH: npath.fromPortablePath(eventPath),
          },
        });
      }),
    );

    test(
      `it should not enable --refresh-lockfile --immutable if the GH environment file is weird`,
      makeTemporaryEnv({
        dependencies: {
          [`one-fixed-dep`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        const lockfilePath = ppath.join(path, Filename.lockfile);
        const lockfileContent = await xfs.readFilePromise(lockfilePath, `utf8`);
        const modifiedLockfile = lockfileContent.replace(/"no-deps": "1\.0\.0"/, `"no-deps": "2.0.0"`);
        await xfs.writeFilePromise(lockfilePath, modifiedLockfile);

        const eventPath = ppath.join(path, `github-event-file.json`);
        await xfs.writeJsonPromise(eventPath, {
          hello: `world`,
        });

        await run(`install`);

        await run(`install`, {
          env: {
            GITHUB_ACTIONS: `true`,
            GITHUB_EVENT_NAME: `pull_request`,
            GITHUB_EVENT_PATH: npath.fromPortablePath(eventPath),
          },
        });
      }),
    );

    test(
      `it should accept to add files to the cache when using --immutable without --immutable-cache`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        await xfs.removePromise(ppath.join(path, `.yarn/cache`));

        await run(`install`, `--immutable`);
      }),
    );

    test(
      `it should refuse to create a cache when using --immutable-cache`,
      makeTemporaryEnv({
        dependencies: {},
      }, async ({path, run, source}) => {
        await expect(run(`install`, `--immutable-cache`)).rejects.toThrowError(/Cache path does not exist/);
      }),
    );

    test(
      `it should refuse to add files to the cache when using --immutable-cache`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        // Ensure the cache directory exists
        await xfs.mkdirPromise(ppath.join(path, `.yarn/cache`), {recursive: true});
        await expect(run(`install`, `--immutable-cache`)).rejects.toThrow(/Cache entry required but missing|cache is immutable/);
      }),
    );

    test(
      `it should refuse to add files to the cache when using --immutable-cache, even when the lockfile is good`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        // Empty, rather than remove the cache
        await xfs.removePromise(ppath.join(path, `.yarn/cache`));
        await xfs.mkdirPromise(ppath.join(path, `.yarn/cache`), {recursive: true});

        await expect(run(`install`, `--immutable-cache`)).rejects.toThrow(/Cache entry required but missing|cache is immutable/);
      }),
    );

    test(
      `it should refuse to remove files from the cache when using --immutable-cache`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        await xfs.writeJsonPromise(ppath.join(path, Filename.manifest), {
          dependencies: {},
        });

        await expect(run(`install`, `--immutable-cache`)).rejects.toThrow(/Cache entry required but missing|cache is immutable/);
      }),
    );

    test(
      `it should refetch the cache files from the remote source when using --check-cache`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        const requests = await tests.startRegistryRecording(async () => {
          await run(`install`, `--check-cache`);
        });

        expect(requests).toEqual(expect.arrayContaining([expect.objectContaining({
          type: tests.RequestType.PackageTarball,
          localName: `no-deps`,
          version: `1.0.0`,
        })]));
      }),
    );

    test(
      `reports warning if published binary field is a path but no package name is set`,
      makeTemporaryEnv(
        {
          bin: `./bin/cli.js`,
        },
        async ({path, run, source}) => {
          const {stdout} = await run(`install`);

          expect(stdout).toContain(`root-workspace-0b6124: String bin field, but no attached package name`);
        },
      ),
    );

    test(
      `displays validation issues of nested workspaces`,
      makeTemporaryEnv(
        {
          workspaces: [`packages`],
        },
        async ({path, run, source}) => {
          await xfs.mkdirPromise(ppath.join(path, `packages`), {recursive: true});
          await xfs.mkdirPromise(ppath.join(path, `packages/package-a`), {recursive: true});

          await xfs.writeJsonPromise(ppath.join(path, `packages`, Filename.manifest), {
            workspaces: [`package-a`],
          });

          await xfs.writeJsonPromise(ppath.join(path, `packages/package-a`, Filename.manifest), {
            bin: `./bin/cli.js`,
          });

          await expect(run(`install`)).resolves.toMatchObject({
            stdout: expect.stringContaining(`package-a-ddd35d: String bin field, but no attached package name`),
          });
        },
      ),
    );

    test(
      `should not build virtual workspaces`,
      makeTemporaryEnv(
        {
          workspaces: [`workspace`],
          dependencies: {
            foo: `workspace:*`,
            'no-deps': `*`,
          },
        },
        async ({path, run, source}) => {
          await xfs.mkdirPromise(ppath.join(path, `workspace`));

          await xfs.writeJsonPromise(ppath.join(path, `workspace`, Filename.manifest), {
            name: `foo`,
            scripts: {
              postinstall: `echo "foo"`,
            },
            peerDependencies: {
              'no-deps': `*`,
            },
          });

          const {stdout} = await run(`install`);

          expect(stdout).toContain(`foo@workspace:foo must be built`);
          expect(stdout).not.toMatch(/foo@virtual:.* must be built/);
          // Should only build once even though the workspace is virtualized
          // through its peer dependency on no-deps.
          expect(stdout.match(/foo@workspace:foo must be built/g)?.length ?? 0).toEqual(1);
        },
      ),
    );

    test(
      `should only print one error message for failed builds`,
      makeTemporaryEnv(
        {
          scripts: {
            postinstall: `exit 1`,
          },
        },
        async ({path, run, source}) => {
          let code;
          let stdout;

          try {
            ({code, stdout} = await run(`install`));
          } catch (error) {
            ({code, stdout} = error);
          }

          expect(code).toEqual(1);
          expect(stdout.match(/couldn't be built successfully/g).length).toEqual(1);
        },
      ),
    );

    test(
      `should not continue running build scripts if one of them fails`,
      makeTemporaryEnv(
        {
          scripts: {
            preinstall: `exit 1`,
            postinstall: `echo 'foo'`,
          },
        },
        async ({path, run, source}) => {
          await expect(run(`install`, `--inline-builds`)).rejects.toMatchObject({
            code: 1,
            stdout: expect.not.stringContaining(`foo`),
          });
        },
      ),
    );

    test(
      `should not mark package as built if any of its scripts fails`,
      makeTemporaryEnv(
        {
          scripts: {
            preinstall: `echo 'foo'`,
            postinstall: `exit 1`,
          },
        },
        async ({path, run, source}) => {
          await expect(run(`install`, `--inline-builds`)).rejects.toMatchObject({
            code: 1,
            stdout: expect.stringContaining(`foo`),
          });

          await expect(run(`install`, `--inline-builds`)).rejects.toMatchObject({
            code: 1,
            stdout: expect.stringContaining(`foo`),
          });
        },
      ),
    );

    test(
      `should not duplicate the build log output on --inline-builds when a build fails`,
      makeTemporaryEnv(
        {
          scripts: {
            postinstall: `echo MARKER_FROM_FAILED_BUILD && exit 1`,
          },
        },
        async ({path, run, source}) => {
          let stdout = ``;
          try {
            await run(`install`, `--inline-builds`);
          } catch (error: any) {
            stdout = error.stdout;
          }

          // Before the fix, both `emit_success_log` and
          // `ChildProcessFailedWithLog` would attach the build log to
          // the install summary, so the report's end-of-run section
          // would dump *two* log files for the same failure (each one
          // containing its own `=== STDOUT ===` header). After the fix
          // only `ChildProcessFailedWithLog`'s log survives.
          const occurrences = (stdout.match(/=== STDOUT ===/g) ?? []).length;
          expect(occurrences).toEqual(1);
        },
      ),
    );

    test(
      `should wait for direct dependencies to finish building`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          'packages/foo': {
            name: `foo`,
            dependencies: {
              bar: `workspace:*`,
            },
            scripts: {
              postinstall: `node -e "require('bar')"`,
            },
          },
          'packages/bar': {
            name: `bar`,
            scripts: {
              postinstall: `sleep 5 && node -e "fs.writeFileSync('index.js', '')"`,
            },
          },
        },
        async ({path, run, source}) => {
          await expect(run(`install`, `--inline-builds`)).resolves.toMatchObject({
            code: 0,
          });
        },
      ),
    );

    test(
      `should wait for indirect dependencies to finish building`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          'packages/foo': {
            name: `foo`,
            dependencies: {
              bar: `workspace:*`,
            },
            scripts: {
              postinstall: `node -e "require('bar')"`,
            },
          },
          'packages/bar': {
            name: `bar`,
            dependencies: {
              baz: `workspace:*`,
            },
          },
          'packages/baz': {
            name: `baz`,
            scripts: {
              postinstall: `sleep 5 && node -e "fs.writeFileSync('index.js', '')"`,
            },
          },
        },
        async ({path, run, source}) => {
          await xfs.writeFilePromise(ppath.join(path, `packages/bar/index.js`), `require('baz')`);
          await expect(run(`install`, `--inline-builds`)).resolves.toMatchObject({
            code: 0,
          });
        },
      ),
    );

    test(
      `should wait for virtual workspace dependencies to finish building`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          'packages/foo': {
            name: `foo`,
            dependencies: {
              bar: `workspace:*`,
            },
            scripts: {
              postinstall: `node -e "require('bar')"`,
            },
          },
          'packages/bar': {
            name: `bar`,
            peerDependencies: {
              'no-deps': `*`,
            },
            scripts: {
              postinstall: `sleep 5 && node -e "fs.writeFileSync('index.js', '')"`,
            },
          },
        },
        async ({path, run, source}) => {
          await expect(run(`install`, `--inline-builds`)).resolves.toMatchObject({
            code: 0,
          });
        },
      ),
    );

    test(
      `should support a self-referencing build dependency`,
      makeTemporaryEnv(
        {
          name: `foo`,
          dependencies: {
            'no-deps': `1.0.0`,
          },
          scripts: {
            postinstall: `echo foo`,
          },
        },
        async ({path, run, source}) => {
          await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
            packageExtensions: {
              'no-deps@*': {
                dependencies: {
                  foo: `workspace:*`,
                },
              },
            },
          });

          await expect(run(`install`, `--inline-builds`)).resolves.toMatchObject({
            code: 0,
          });
        },
      ),
    );

    test(
      `should support a self-referencing virtual workspace build dependency`,
      makeTemporaryMonorepoEnv(
        {
          workspaces: [`packages/*`],
        },
        {
          'packages/foo': {
            name: `foo`,
            peerDependencies: {
              'no-deps': `1.0.0`,
            },
            dependencies: {
              bar: `workspace:*`,
            },
            scripts: {
              postinstall: `echo foo`,
            },
          },
          'packages/bar': {
            name: `bar`,
            dependencies: {
              foo: `workspace:*`,
            },
          },
        },
        async ({path, run, source}) => {
          await expect(run(`install`, `--inline-builds`)).resolves.toMatchObject({
            code: 0,
          });
        },
      ),
    );

    test(
      `it should print a warning when using \`enableScripts: false\``,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps-scripted`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
          enableScripts: false,
        });

        const {stdout} = await run(`install`, `--inline-builds`, {
          env: {
            YARN_ENABLE_SCRIPTS: `false`,
          },
        });
        expect(stdout).toMatch(/lists build scripts, but its build has been explicitly disabled/g);
      }),
    );

    test(
      `it should print an info when \`dependenciesMeta[].built: false\`, even when using using \`enableScripts: false\``,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps-scripted`]: `1.0.0`,
        },
        dependenciesMeta: {
          'no-deps-scripted': {
            built: false,
          },
        },
      }, async ({path, run, source}) => {
        await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
          enableScripts: false,
        });

        const {stdout} = await run(`install`, `--inline-builds`, {
          env: {
            YARN_ENABLE_SCRIPTS: `false`,
          },
        });

        expect(stdout).toMatch(/lists build scripts, but its build has been explicitly disabled/g);
      }),
    );

    test(
      `it should throw a proper error if not find any locator`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await xfs.mkdirPromise(ppath.join(path, `non-workspace`));

        await xfs.writeJsonPromise(ppath.join(path, `non-workspace`, Filename.manifest), {
          name: `non-workspace`,
        });

        await expect(run(`install`, {cwd: ppath.join(path, `non-workspace`)})).rejects.toMatchObject({
          code: 1,
          stdout: expect.stringMatching(/The nearest package directory \(.+\) doesn't seem to be part of the project declared in .+\./g),
        });
      }),
    );

    test(
      `it should fetch only required packages when using \`--mode=update-lockfile\``,
      makeTemporaryEnv({
        dependencies: {
          [`one-fixed-dep`]: `1.0.0`,
          [`no-deps`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);

        await xfs.writeJsonPromise(ppath.join(path, Filename.manifest), {
          dependencies: {
            [`one-fixed-dep`]: `1.0.0`,
            [`no-deps`]: `2.0.0`,
          },
        });

        await xfs.removePromise(ppath.join(path, `.yarn/cache`));
        await xfs.mkdirPromise(ppath.join(path, `.yarn/cache`), {recursive: true});

        await expect(tests.startRegistryRecording(async () => {
          await run(`install`, `--mode=update-lockfile`);
        })).resolves.toEqual([
          {
            type: tests.RequestType.PackageInfo,
            scope: undefined,
            localName: `no-deps`,
          },
          {
            type: tests.RequestType.PackageTarball,
            scope: undefined,
            localName: `no-deps`,
            version: `2.0.0`,
          },
        ]);

        const cacheAfter = await xfs.readdirPromise(ppath.join(path, `.yarn/cache`));
        expect(cacheAfter.find(entry => entry.includes(`no-deps-npm-2.0.0`))).toBeDefined();
      }),
    );

    test(
      `it should disable immutable installs when using \`--mode=update-lockfile\``,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, async ({path, run}) => {
        await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
          enableImmutableInstalls: true,
        });

        const {stdout} = await run(`install`, `--mode=update-lockfile`);
        expect(stdout).not.toMatch(/The lockfile would have been created by this install/g);
      }),
    );

    test(
      `it should throw when \`--immutable\` or \`--immutable-cache\` is specified with \`--mode=update-lockfile\``,
      makeTemporaryEnv({}, async ({path, run}) => {
        await expect(run(`install`, `--mode=update-lockfile`, `--immutable`)).rejects.toMatchObject({
          code: 1,
          stdout: expect.stringMatching(/--immutable and --immutable-cache cannot be used with --mode=update-lockfile/g),
        });
        await expect(run(`install`, `--mode=update-lockfile`, `--immutable-cache`)).rejects.toMatchObject({
          code: 1,
          stdout: expect.stringMatching(/--immutable and --immutable-cache cannot be used with --mode=update-lockfile/g),
        });
        await expect(run(`install`, `--mode=update-lockfile`, `--immutable`, `--immutable-cache`)).rejects.toMatchObject({
          code: 1,
          stdout: expect.stringMatching(/--immutable and --immutable-cache cannot be used with --mode=update-lockfile/g),
        });
      }),
    );

    test(
      `it should support registries that return escaped JSON`,
      makeTemporaryEnv({
        dependencies: {
          [`one-range-dep-escaped`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`install`);
      }),
    );
  });
});
