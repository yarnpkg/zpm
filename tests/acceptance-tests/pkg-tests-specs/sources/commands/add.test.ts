import {Filename, PortablePath, ppath, xfs} from '@yarnpkg/fslib';
import {parseSyml}                          from '@yarnpkg/parsers';
import {tests, yarn}                        from 'pkg-tests-core';

describe(`Commands`, () => {
  describe(`add`, () => {
    test(
      `it should add a new regular dependency to the current project (explicit semver)`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, `no-deps@1.0.0`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: `1.0.0`,
          },
        });
      }),
    );

    test.each([
      [`no-deps@^1.0.0`, [], `^1.0.0`],
      [`no-deps@~1.0.0`, [], `~1.0.0`],
      [`no-deps@1`, [], `1`],
      [`no-deps@1.x`, [], `1.x`],
      [`no-deps@>=1.0.0 <1.1.0`, [], `>=1.0.0 <1.1.0`],
      [`no-deps@^1.0.0`, [`-E`], `^1.0.0`],
      [`no-deps@^1.0.0`, [`-T`], `^1.0.0`],
      [`no-deps@~1.0.0`, [`-C`], `~1.0.0`],
      [`no-deps@1.0.0`, [`-T`], `1.0.0`],
      [`no-deps@1.0.0`, [`-C`], `1.0.0`],
    ])(
      `it should keep explicit semver ranges as-is (%s %j)`,
      (request, flags, expected) => makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, request, ...flags);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: expected,
          },
        });
      })(),
    );

    test(
      `it should keep explicit semver ranges as-is regardless of defaultSemverRangePrefix`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`config`, `set`, `defaultSemverRangePrefix`, `~`);
        await run(`add`, `no-deps@^1.0.0`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: `^1.0.0`,
          },
        });
      }),
    );

    test(
      `it should install the highest version matching an explicit semver range`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, `no-deps@^1.0.0`);

        await expect(source(`require('no-deps')`)).resolves.toMatchObject({
          name: `no-deps`,
          version: `1.1.0`,
        });
      }),
    );

    test(
      `it should keep explicit semver ranges as-is (npm: protocol)`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, `no-deps@npm:^1.0.0`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            // The default protocol may be omitted, but the range must be the requested one
            [`no-deps`]: expect.stringMatching(/^(npm:)?\^1\.0\.0$/),
          },
        });
      }),
    );

    test(
      `it should keep explicit semver ranges as-is (alias)`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, `foo@npm:no-deps@^1.0.0`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`foo`]: `npm:no-deps@^1.0.0`,
          },
        });

        await expect(source(`require('foo')`)).resolves.toMatchObject({
          name: `no-deps`,
          version: `1.1.0`,
        });
      }),
    );

    test(
      `it should keep the alias when resolving a tag (alias)`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, `foo@npm:no-deps@latest`, `-T`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`foo`]: `npm:no-deps@~2.0.0`,
          },
        });

        await expect(source(`require('foo')`)).resolves.toMatchObject({
          name: `no-deps`,
          version: `2.0.0`,
        });
      }),
    );

    test(
      `it should apply the modifier flags to resolved tags`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, `no-deps@latest`, `-T`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: `~2.0.0`,
          },
        });
      }),
    );

    test(
      `it should keep catalog references as-is`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await yarn.writeConfiguration(path, {
          catalog: {
            [`no-deps`]: `^1.0.0`,
          },
        });

        await run(`add`, `no-deps@catalog:`, `-E`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: `catalog:`,
          },
        });

        await expect(source(`require('no-deps')`)).resolves.toMatchObject({
          name: `no-deps`,
          version: `1.1.0`,
        });
      }),
    );

    test(
      `it should add a new regular dependency to the current project (implicit caret)`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, `no-deps`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: `^2.0.0`,
          },
        });
      }),
    );

    test(
      `it should add a new regular dependency to the current project (explicit tilde)`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, `no-deps`, `-T`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: `~2.0.0`,
          },
        });
      }),
    );

    test(
      `it should add a new regular dependency to the current project (explicit exact)`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, `no-deps`, `-E`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: `2.0.0`,
          },
        });
      }),
    );

    test(
      `it should add a new regular dependency to the current project (explicit caret)`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`config`, `set`, `defaultSemverRangePrefix`, `~`);
        await run(`add`, `no-deps`, `-C`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: `^2.0.0`,
          },
        });
      }),
    );

    test(
      `it should add a new regular dependency to the current project (tilde via defaultSemverRangePrefix)`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`config`, `set`, `defaultSemverRangePrefix`, `~`);
        await run(`add`, `no-deps`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: `~2.0.0`,
          },
        });
      }),
    );

    test(
      `it should add a new regular dependency to the current project (exact via defaultSemverRangePrefix)`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`config`, `set`, `defaultSemverRangePrefix`, ``);
        await run(`add`, `no-deps`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: `2.0.0`,
          },
        });
      }),
    );

    test(
      `it should add a new regular dependency to the current project (unnamed path)`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        const packagePath = await tests.getPackageDirectoryPath(`no-deps`, `1.0.0`);

        await run(`add`, packagePath);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: packagePath,
          },
        });
      }),
    );

    test(
      `it should add a new development dependency to the current project`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, `no-deps`, `-D`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          devDependencies: {
            [`no-deps`]: `^2.0.0`,
          },
        });
      }),
    );

    test(
      `it should add a new regular dependency to the current project (resolved tag)`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, `no-deps@latest`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: `^2.0.0`,
          },
        });
      }),
    );

    test(
      `it should add a new regular dependency to the current project (resolved backward tag)`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, `no-deps-backward-tags@rc`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps-backward-tags`]: `1.0.0-rc.1`,
          },
        });
      }),
    );

    test(
      `it should add a new regular dependency to the current project (fixed tag)`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, `-F`, `no-deps@latest`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: `latest`,
          },
        });
      }),
    );

    test(
      `it should upgrade the existing regular dependency in the current project (--prefer-dev)`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`add`, `no-deps`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: `^2.0.0`,
          },
        });
      }),
    );

    test(
      `it should upgrade the existing regular dependency in the current project (implicit)`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`add`, `no-deps`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: `^2.0.0`,
          },
        });
      }),
    );

    test(
      `it should upgrade the existing development dependency in the current project (-D)`,
      makeTemporaryEnv({
        devDependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`add`, `no-deps`, `-D`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          devDependencies: {
            [`no-deps`]: `^2.0.0`,
          },
        });
      }),
    );

    test(
      `it should upgrade the existing development dependency in the current project (--prefer-dev)`,
      makeTemporaryEnv({
        devDependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`add`, `no-deps`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          devDependencies: {
            [`no-deps`]: `^2.0.0`,
          },
        });
      }),
    );

    test(
      `it should upgrade the existing development dependency in the current project (implicit)`,
      makeTemporaryEnv({
        devDependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await run(`add`, `no-deps`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          devDependencies: {
            [`no-deps`]: `^2.0.0`,
          },
        });
      }),
    );

    test(
      `it should not upgrade the existing dependency in the current project for preferReuse`,
      makeTemporaryEnv({
        devDependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, {preferReuse: true}, async ({path, run, source}) => {
        await run(`add`, `no-deps`);

        await expect(xfs.readJsonPromise(`${path}/package.json` as PortablePath)).resolves.toMatchObject({
          devDependencies: {
            [`no-deps`]: `1.0.0`,
          },
        });
      }),
    );

    test(
      `it should add a new peer dependency to the current project`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, `no-deps`, `-P`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          peerDependencies: {
            [`no-deps`]: `*`,
          },
        });
      }),
    );

    test(
      `it should throw an error when existing regular dependency is added using --dev`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, `no-deps`);
        await expect(
          run(`add`, `no-deps`, `-D`),
        ).rejects.toThrowError(/already listed as a regular dependency/);
      }),
    );

    test(
      `it should throw an error when existing regular dependency is added using --peer`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, `no-deps`);
        await expect(
          run(`add`, `no-deps`, `-P`),
        ).rejects.toThrowError(/already listed as a regular dependency/);
      }),
    );

    test(
      `it should throw an error when existing peer dependency is added without --peer|--dev`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, `no-deps`, `-P`);
        await expect(
          run(`add`, `no-deps`),
        ).rejects.toThrowError(/already listed as a peer dependency/);
      }),
    );

    test(
      `it should add an existing development dependency as peer dependency when added using --peer `,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, `no-deps`, `-D`);
        await run(`add`, `no-deps`, `-P`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          devDependencies: {
            [`no-deps`]: `^2.0.0`,
          },
          peerDependencies: {
            [`no-deps`]: `*`,
          },
        });
      }),
    );

    test(
      `it should add a new peer dependency and a new development dependency to the current project when using --peer and --dev together`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, `no-deps`, `-D`, `-P`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          devDependencies: {
            [`no-deps`]: `^2.0.0`,
          },
          peerDependencies: {
            [`no-deps`]: `*`,
          },
        });
      }),
    );

    test(
      `it should add a node-gyp dependency to the lockfile if a script uses it`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, `inject-node-gyp`);

        const content = await xfs.readFilePromise(`${path}/yarn.lock` as PortablePath, `utf8`);
        const lock = parseSyml(content);

        const expectedLockfile = tests.FEATURE_CHECKS.jsonLockfile ? {
          entries: {
            [`inject-node-gyp@npm:^1.0.0`]: {
              resolution: {
                dependencies: {
                  [`node-gyp`]: `*`,
                },
              },
            },
          },
        } : {
          [`inject-node-gyp@npm:^1.0.0`]: {
            dependencies: {
              [`node-gyp`]: `npm:latest`,
            },
          },
        };

        expect(lock).toMatchObject(expectedLockfile);
      }),
    );

    test(
      `it should add a node-gyp dependency to the lockfile if a development script uses it`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, `node-gyp-dev-scripts`);

        const lock = parseSyml(await xfs.readFilePromise(`${path}/yarn.lock` as PortablePath, `utf8`));
        const entry = tests.FEATURE_CHECKS.jsonLockfile
          ? lock.entries[`node-gyp-dev-scripts@npm:^1.0.0`].resolution
          : lock[`node-gyp-dev-scripts@npm:^1.0.0`];

        expect(Object.keys(entry.dependencies ?? {})).toEqual([`node-gyp`]);
      }),
    );

    test(
      `it shouldn't add a node-gyp dependency to the lockfile if only a development script uses it and gypfile is false`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, `gypfile-false-dev-scripts`);

        const lock = parseSyml(await xfs.readFilePromise(`${path}/yarn.lock` as PortablePath, `utf8`));
        const entry = tests.FEATURE_CHECKS.jsonLockfile
          ? lock.entries[`gypfile-false-dev-scripts@npm:^1.0.0`].resolution
          : lock[`gypfile-false-dev-scripts@npm:^1.0.0`];

        expect(Object.keys(entry.dependencies ?? {})).toEqual([]);
        expect(Object.keys(tests.FEATURE_CHECKS.jsonLockfile ? lock.entries : lock).some(key => key.startsWith(`node-gyp@`))).toEqual(false);
      }),
    );

    test(
      `it should add a node-gyp dependency to the lockfile if an install script uses it, even if gypfile is false`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`add`, `gypfile-false-install-script`);

        const lock = parseSyml(await xfs.readFilePromise(`${path}/yarn.lock` as PortablePath, `utf8`));
        const entry = tests.FEATURE_CHECKS.jsonLockfile
          ? lock.entries[`gypfile-false-install-script@npm:^1.0.0`].resolution
          : lock[`gypfile-false-install-script@npm:^1.0.0`];

        expect(Object.keys(entry.dependencies ?? {})).toEqual([`node-gyp`]);
      }),
    );

    test(
      `it should suggest a workspace if it would match the request (explicit path)`,
      makeTemporaryEnv({
        private: true,
        workspaces: [`packages/*`],
      }, async ({path, run, source}) => {
        await xfs.mkdirpPromise(`${path}/packages/no-deps` as PortablePath);

        await xfs.writeJsonPromise(`${path}/packages/no-deps/package.json` as PortablePath, {
          name: `no-deps`,
        });

        await run(`add`, `no-deps@workspace:packages/no-deps`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: `workspace:packages/no-deps`,
          },
        });
      }),
    );

    test(
      `it should suggest a workspace if it would match the request (explicit semver)`,
      makeTemporaryEnv({
        private: true,
        workspaces: [`packages/*`],
      }, async ({path, run, source}) => {
        await xfs.mkdirpPromise(`${path}/packages/no-deps` as PortablePath);

        await xfs.writeJsonPromise(`${path}/packages/no-deps/package.json` as PortablePath, {
          name: `no-deps`,
          version: `1.0.0`,
        });

        await run(`add`, `no-deps@workspace:1.0.0`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: `workspace:1.0.0`,
          },
        });
      }),
    );

    test(
      `it should suggest a workspace if it would match the request (implicit caret)`,
      makeTemporaryEnv({
        private: true,
        workspaces: [`packages/*`],
      }, async ({path, run, source}) => {
        await xfs.mkdirpPromise(`${path}/packages/no-deps` as PortablePath);

        await xfs.writeJsonPromise(`${path}/packages/no-deps/package.json` as PortablePath, {
          name: `no-deps`,
        });

        await run(`add`, `no-deps`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: `workspace:^`,
          },
        });
      }),
    );

    test(
      `it should suggest a workspace if it would match the request (explicit tilde)`,
      makeTemporaryEnv({
        private: true,
        workspaces: [`packages/*`],
      }, async ({path, run, source}) => {
        await xfs.mkdirpPromise(`${path}/packages/no-deps` as PortablePath);

        await xfs.writeJsonPromise(`${path}/packages/no-deps/package.json` as PortablePath, {
          name: `no-deps`,
        });

        await run(`add`, `no-deps`, `-T`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: `workspace:~`,
          },
        });
      }),
    );

    test(
      `it should suggest a workspace if it would match the request (explicit exact)`,
      makeTemporaryEnv({
        private: true,
        workspaces: [`packages/*`],
      }, async ({path, run, source}) => {
        await xfs.mkdirpPromise(`${path}/packages/no-deps` as PortablePath);

        await xfs.writeJsonPromise(`${path}/packages/no-deps/package.json` as PortablePath, {
          name: `no-deps`,
        });

        await run(`add`, `no-deps`, `-E`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: `workspace:*`,
          },
        });
      }),
    );

    test(
      `it should suggest a workspace if it would match the request (explicit caret)`,
      makeTemporaryEnv({
        private: true,
        workspaces: [`packages/*`],
      }, async ({path, run, source}) => {
        await xfs.mkdirpPromise(`${path}/packages/no-deps` as PortablePath);

        await xfs.writeJsonPromise(`${path}/packages/no-deps/package.json` as PortablePath, {
          name: `no-deps`,
        });

        await run(`config`, `set`, `defaultSemverRangePrefix`, `~`);
        await run(`add`, `no-deps`, `-C`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: `workspace:^`,
          },
        });
      }),
    );

    test(
      `it should suggest a workspace if it would match the request (tilde via defaultSemverRangePrefix)`,
      makeTemporaryEnv({
        private: true,
        workspaces: [`packages/*`],
      }, async ({path, run, source}) => {
        await xfs.mkdirpPromise(`${path}/packages/no-deps` as PortablePath);

        await xfs.writeJsonPromise(`${path}/packages/no-deps/package.json` as PortablePath, {
          name: `no-deps`,
        });

        await run(`config`, `set`, `defaultSemverRangePrefix`, `~`);
        await run(`add`, `no-deps`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: `workspace:~`,
          },
        });
      }),
    );

    test(
      `it should suggest a workspace if it would match the request (exact via defaultSemverRangePrefix)`,
      makeTemporaryEnv({
        private: true,
        workspaces: [`packages/*`],
      }, async ({path, run, source}) => {
        await xfs.mkdirpPromise(`${path}/packages/no-deps` as PortablePath);

        await xfs.writeJsonPromise(`${path}/packages/no-deps/package.json` as PortablePath, {
          name: `no-deps`,
        });

        await run(`config`, `set`, `defaultSemverRangePrefix`, ``);
        await run(`add`, `no-deps`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: `workspace:*`,
          },
        });
      }),
    );

    test(
      `it shouldn't suggest a workspace to fulfill its own dependency`,
      makeTemporaryEnv({
        name: `no-deps`,
      }, async ({path, run, source}) => {
        await run(`add`, `no-deps`);

        await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
          dependencies: {
            [`no-deps`]: `^2.0.0`,
          },
        });
      }),
    );

    test(`it should clean the cache when cache lives inside the project`, makeTemporaryEnv({
      dependencies: {
        [`no-deps`]: `1.0.0`,
      },
    }, async ({path, run, source}) => {
      await run(`install`);

      const preUpgradeCache = await xfs.readdirPromise(`${path}/.yarn/cache` as PortablePath);

      expect(preUpgradeCache.find(entry => entry.includes(`no-deps-npm-1.0.0`))).toBeDefined();

      await run(`add`, `no-deps@2.0.0`);

      const postUpgradeCache = await xfs.readdirPromise(`${path}/.yarn/cache` as PortablePath);

      expect(postUpgradeCache.find(entry => entry.includes(`no-deps-npm-1.0.0`))).toBeUndefined();
      expect(postUpgradeCache.find(entry => entry.includes(`no-deps-npm-2.0.0`))).toBeDefined();
    }));

    test(`it should not clean the cache when cache lives inside the project but global cache is set`, makeTemporaryEnv({
      dependencies: {
        [`no-deps`]: `1.0.0`,
      },
    }, async ({path, run, source}) => {
      const env = {
        YARN_ENABLE_GLOBAL_CACHE: `true`,
        YARN_GLOBAL_FOLDER: `${path}/global`,
      };

      await run(`install`, {env});

      const preUpgradeCache = await xfs.readdirPromise(`${path}/global/cache` as PortablePath);

      expect(preUpgradeCache.find(entry => entry.includes(`no-deps-npm-1.0.0`))).toBeDefined();

      await run(`add`, `no-deps@2.0.0`, {env});

      const postUpgradeCache = await xfs.readdirPromise(`${path}/global/cache` as PortablePath);

      expect(postUpgradeCache.find(entry => entry.includes(`no-deps-npm-1.0.0`))).toBeDefined();
      expect(postUpgradeCache.find(entry => entry.includes(`no-deps-npm-2.0.0`))).toBeDefined();
    }));

    test(`it should not clean the cache when cache lives outside the project`, makeTemporaryEnv({
      dependencies: {
        [`no-deps`]: `1.0.0`,
      },
    }, async ({path, run, source}) => {
      const sharedCachePath = await xfs.mktempPromise();
      const env = {
        YARN_ENABLE_GLOBAL_CACHE: `false`,
        YARN_CACHE_FOLDER: sharedCachePath,
      };

      let cacheContent;

      await run(`install`, {env});

      cacheContent = await xfs.readdirPromise(sharedCachePath);

      expect(cacheContent.find(entry => entry.includes(`no-deps-npm-1.0.0`))).toBeDefined();

      await run(`add`, `no-deps@2.0.0`, {env});

      cacheContent = await xfs.readdirPromise(sharedCachePath);

      expect(cacheContent.find(entry => entry.includes(`no-deps-npm-1.0.0`))).toBeDefined();
      expect(cacheContent.find(entry => entry.includes(`no-deps-npm-2.0.0`))).toBeDefined();
    }));
  });
});
