import {Filename, PortablePath, ppath, xfs} from '@yarnpkg/fslib';
import {exec, tests, yarn}                  from 'pkg-tests-core';

const {setPackageWhitelist} = tests;

type Manifest = Record<string, any>;

async function writeWorkspace(path: PortablePath, name: string, manifest: Manifest = {}) {
  const workspacePath = ppath.join(path, `packages/${name}` as PortablePath);

  await xfs.mkdirpPromise(workspacePath);
  await xfs.writeJsonPromise(ppath.join(workspacePath, Filename.manifest), {
    name,
    version: `1.0.0`,
    ...manifest,
  });
}

async function readLockfile(path: PortablePath) {
  return await xfs.readJsonPromise(ppath.join(path, Filename.lockfile));
}

async function commit(path: PortablePath, message: string) {
  await exec.execFile(`git`, [`add`, `-A`], {cwd: path});
  await exec.execFile(`git`, [`commit`, `-m`, message], {cwd: path});
}

async function initRepository(path: PortablePath) {
  await xfs.writeFilePromise(ppath.join(path, `.gitignore` as PortablePath), [
    `.yarn\n`,
    `.pnp.*\n`,
    `node_modules\n`,
  ].join(``));

  await exec.execGitInit({cwd: path});
  await commit(path, `First commit`);
}

async function getChangedWorkspaces(run: (...args: Array<any>) => Promise<{stdout: string}>) {
  const {stdout} = await run(`debug`, `print-changed-workspaces`, `--since`, `HEAD`);

  return stdout.split(`\n`).filter(line => line.length > 0).sort();
}

async function getTreeHashes(run: (...args: Array<any>) => Promise<{stdout: string}>) {
  const {stdout} = await run(`workspaces`, `list`, `--json`, `--tree-hash`);

  const entries = stdout.split(`\n`).filter(line => line.length > 0).map(line => {
    const {name, treeHash} = JSON.parse(line);
    return [name, treeHash];
  });

  return Object.fromEntries(entries);
}

const makeMonorepoEnv = (cb: Parameters<typeof makeTemporaryEnv>[1]) => makeTemporaryEnv({
  private: true,
  workspaces: [`packages/*`],
}, cb);

describe(`Features`, () => {
  describe(`Workspace change detection`, () => {
    describe(`Lockfile`, () => {
      test(
        `it should store the workspace hashes within the project section`,
        makeMonorepoEnv(async ({path, run}) => {
          await writeWorkspace(path, `workspace-a`, {dependencies: {[`no-deps`]: `1.0.0`}});
          await writeWorkspace(path, `workspace-b`);

          await run(`install`);

          const lockfile = await readLockfile(path);

          expect(lockfile).not.toHaveProperty(`workspaces`);

          // The catalogs, overrides, and extensions are only there when the project has some
          expect(Object.keys(lockfile.project)).toEqual([`workspaces`]);

          expect(Object.keys(lockfile.project.workspaces)).toEqual([
            `root-workspace`,
            `workspace-a`,
            `workspace-b`,
          ]);

          // Neither of those workspaces has any dependency
          expect(lockfile.project.workspaces[`workspace-b`]).toEqual(lockfile.project.workspaces[`root-workspace`]);
          expect(lockfile.project.workspaces[`workspace-a`]).not.toEqual(lockfile.project.workspaces[`workspace-b`]);
        }),
      );

      test(
        `it shouldn't update the workspace hashes when only transitive dependencies change`,
        makeMonorepoEnv(async ({path, run}) => {
          await writeWorkspace(path, `workspace-a`, {dependencies: {[`one-range-dep`]: `1.0.0`}});

          await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
            await run(`install`);
          });

          const before = await readLockfile(path);
          expect(before.entries[`no-deps@npm:^1.0.0`].resolution.resolution).toEqual(`no-deps@npm:1.0.0`);

          await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`, `1.1.0`])]]), async () => {
            await run(`up`, `-R`, `no-deps`);
          });

          const after = await readLockfile(path);
          expect(after.entries[`no-deps@npm:^1.0.0`].resolution.resolution).toEqual(`no-deps@npm:1.1.0`);

          expect(after.project).toEqual(before.project);
        }),
      );

      test(
        `it should update the workspace hashes when their dependencies change`,
        makeMonorepoEnv(async ({path, run}) => {
          await writeWorkspace(path, `workspace-a`, {dependencies: {[`no-deps`]: `1.0.0`}});
          await writeWorkspace(path, `workspace-b`, {dependencies: {[`no-deps`]: `1.0.0`}});

          await run(`install`);
          const before = await readLockfile(path);

          await writeWorkspace(path, `workspace-a`, {dependencies: {[`no-deps`]: `2.0.0`}});

          await run(`install`);
          const after = await readLockfile(path);

          expect(after.project.workspaces[`workspace-a`]).not.toEqual(before.project.workspaces[`workspace-a`]);
          expect(after.project.workspaces[`workspace-b`]).toEqual(before.project.workspaces[`workspace-b`]);
        }),
      );

      test(
        `it should update the workspace hashes when the catalog entries they reference change`,
        makeMonorepoEnv(async ({path, run}) => {
          await writeWorkspace(path, `workspace-a`, {dependencies: {[`no-deps`]: `catalog:`}});
          await writeWorkspace(path, `workspace-b`, {dependencies: {[`no-deps`]: `1.0.0`}});

          await yarn.writeConfiguration(path, {catalog: {[`no-deps`]: `1.0.0`}});

          await run(`install`);
          const before = await readLockfile(path);

          // Once normalized, both workspaces have the exact same dependencies
          expect(before.project.workspaces[`workspace-a`]).toEqual(before.project.workspaces[`workspace-b`]);

          await yarn.writeConfiguration(path, {catalog: {[`no-deps`]: `2.0.0`}});

          await run(`install`);
          const after = await readLockfile(path);

          expect(after.project.workspaces[`workspace-a`]).not.toEqual(before.project.workspaces[`workspace-a`]);
          expect(after.project.workspaces[`workspace-b`]).toEqual(before.project.workspaces[`workspace-b`]);
        }),
      );

      test(
        `it should store the transient resolutions, but not the workspaces`,
        makeMonorepoEnv(async ({path, run}) => {
          await xfs.mkdirpPromise(ppath.join(path, `vendor/portal` as PortablePath));
          await xfs.writeJsonPromise(ppath.join(path, `vendor/portal/package.json` as PortablePath), {
            name: `portal`,
            version: `1.0.0`,
            dependencies: {[`no-deps`]: `1.0.0`},
          });

          await xfs.mkdirpPromise(ppath.join(path, `vendor/link` as PortablePath));

          await writeWorkspace(path, `workspace-a`, {
            dependencies: {
              [`aliased`]: `npm:no-deps@2.0.0`,
              [`link`]: `link:../../vendor/link`,
              [`portal`]: `portal:../../vendor/portal`,
              [`workspace-b`]: `workspace:^`,
              [`workspace-c`]: `^1.0.0`,
            },
          });

          await writeWorkspace(path, `workspace-b`);
          await writeWorkspace(path, `workspace-c`);

          await yarn.writeConfiguration(path, {enableTransparentWorkspaces: true});

          await run(`install`);

          const lockfile = await readLockfile(path);
          const keys = Object.keys(lockfile.entries);

          expect(keys).toEqual([
            `aliased@npm:no-deps@2.0.0, no-deps@npm:2.0.0`,
            `link@link:../../vendor/link::parent=workspace-a@workspace:workspace-a`,
            `no-deps@npm:1.0.0`,
            `portal@portal:../../vendor/portal::parent=workspace-a@workspace:workspace-a`,
          ]);

          // The lockfile must remain stable despite the transient entries
          // being recomputed by every install
          await xfs.removePromise(ppath.join(path, `.yarn/ignore` as PortablePath));
          await run(`install`, `--immutable`);

          await expect(readLockfile(path)).resolves.toEqual(lockfile);
        }),
      );

      test(
        `it should be able to read back every kind of transient resolution`,
        makeMonorepoEnv(async ({path, run}) => {
          const patch = [
            `diff --git a/index.js b/index.js\n`,
            `--- a/index.js\n`,
            `+++ b/index.js\n`,
            `@@ -1,1 +1,2 @@\n`,
            ` module.exports = require(\`./package.json\`);\n`,
            `+module.exports.patched = true;\n`,
          ].join(``);

          await writeWorkspace(path, `workspace-a`, {
            dependencies: {
              [`aliased`]: `npm:no-deps@1.0.0`,
              [`executed`]: `exec:./genpkg.js`,
              [`folder`]: `file:./vendor/folder`,
              [`linked`]: `link:./vendor/link`,
              [`no-deps`]: `patch:no-deps@1.0.0#./local.patch`,
              [`one-fixed-dep`]: `patch:one-fixed-dep@1.0.0#~/project.patch`,
              [`portal`]: `portal:./vendor/portal`,
              [`tarball`]: `file:./vendor/tarball.tgz`,
            },
          });

          const workspacePath = ppath.join(path, `packages/workspace-a` as PortablePath);

          await xfs.writeFilePromise(ppath.join(workspacePath, `genpkg.js` as PortablePath), [
            `fs.writeFileSync(path.join(execEnv.buildDir, 'package.json'), JSON.stringify({name: 'executed', version: '1.0.0'}));\n`,
          ].join(``));

          for (const name of [`folder`, `portal`]) {
            await xfs.mkdirpPromise(ppath.join(workspacePath, `vendor/${name}` as PortablePath));
            await xfs.writeJsonPromise(ppath.join(workspacePath, `vendor/${name}/package.json` as PortablePath), {
              name,
              version: `1.0.0`,
              dependencies: {[`no-deps`]: `2.0.0`},
            });
          }

          await xfs.mkdirpPromise(ppath.join(workspacePath, `vendor/link` as PortablePath));
          await xfs.copyFilePromise(await tests.getPackageArchivePath(`no-deps`, `1.0.0`), ppath.join(workspacePath, `vendor/tarball.tgz` as PortablePath));

          await xfs.writeFilePromise(ppath.join(workspacePath, `local.patch` as PortablePath), patch);
          await xfs.writeFilePromise(ppath.join(path, `project.patch` as PortablePath), patch);

          await run(`install`);

          const lockfile = await readLockfile(path);

          const protocols = Object.values<any>(lockfile.entries)
            .map(entry => entry.resolution.resolution.match(/^(?:@[^/]+\/)?[^@]+@([a-z]+):/)?.[1])
            .filter((protocol, index, list) => list.indexOf(protocol) === index)
            .sort();

          expect(protocols).toEqual([`exec`, `file`, `link`, `npm`, `patch`, `portal`]);

          // Both of those commands need to parse the lockfile we just generated
          await xfs.removePromise(ppath.join(path, `.yarn/ignore` as PortablePath));
          await run(`install`, `--immutable`);

          await initRepository(path);
          await expect(getChangedWorkspaces(run)).resolves.toEqual([]);

          await expect(readLockfile(path)).resolves.toEqual(lockfile);
        }),
      );

      test(
        `it should only store the catalogs, dependency overrides, and package extensions that are used`,
        makeTemporaryEnv({
          private: true,
          workspaces: [`packages/*`],
          resolutions: {
            [`one-range-dep/no-deps`]: `catalog:`,
            [`one-fixed-dep/no-deps`]: `2.0.0`,
            [`no-deps`]: `1.0.0`,
            [`various-requires/no-deps`]: `1.1.0`,
            [`left-pad`]: `1.0.0`,
          },
        }, async ({path, run}) => {
          await writeWorkspace(path, `workspace-a`, {
            dependencies: {
              [`is-number`]: `catalog:numbers`,
              [`one-range-dep`]: `1.0.0`,
              [`various-requires`]: `1.0.0`,
            },
          });

          await yarn.writeConfiguration(path, {
            catalog: {
              [`left-pad`]: `1.0.0`,
              [`no-deps`]: `1.1.0`,
            },
            catalogs: {
              legacy: {
                [`no-deps`]: `1.0.0`,
              },
              numbers: {
                [`is-number`]: `1.0.0`,
                [`no-deps`]: `2.0.0`,
              },
            },
            packageExtensions: {
              [`one-fixed-dep@*`]: {
                dependencies: {[`left-pad`]: `1.0.0`},
              },
              [`various-requires@*`]: {
                dependencies: {[`no-deps`]: `2.0.0`},
                peerDependenciesMeta: {[`left-pad`]: {optional: true}},
              },
            },
          });

          await run(`install`);

          const lockfile = await readLockfile(path);

          // The catalog entries can be referenced by the workspaces (is-number)
          // or by the overrides (no-deps); the others are left out.
          expect(lockfile.project.catalogs).toEqual({
            default: {[`no-deps`]: `1.1.0`},
            numbers: {[`is-number`]: `1.0.0`},
          });

          // The overrides are otherwise a plain copy of the `resolutions` field, and keep
          // their order (first match wins). Neither one-fixed-dep nor left-pad are in the
          // tree, and the various-requires rule is always shadowed by the no-deps one.
          expect(Object.entries(lockfile.project.dependencyOverrides)).toEqual([
            [`one-range-dep/no-deps`, `catalog:`],
            [`no-deps`, `1.0.0`],
          ]);

          expect(lockfile.project.packageExtensions).toEqual({
            [`various-requires@*`]: {
              dependencies: {[`no-deps`]: `2.0.0`},
              peerDependenciesMeta: {[`left-pad`]: {optional: true}},
            },
          });

          await xfs.removePromise(ppath.join(path, `.yarn/ignore` as PortablePath));
          await run(`install`, `--immutable`);
        }),
      );

      test(
        `it shouldn't update the lockfile when changing rules that aren't used`,
        makeTemporaryEnv({
          private: true,
          workspaces: [`packages/*`],
          resolutions: {[`left-pad`]: `1.0.0`},
        }, async ({path, run}) => {
          await writeWorkspace(path, `workspace-a`, {dependencies: {[`no-deps`]: `catalog:`}});

          await yarn.writeConfiguration(path, {
            catalog: {
              [`left-pad`]: `1.0.0`,
              [`no-deps`]: `1.0.0`,
            },
          });

          await run(`install`);

          const lockfile = await readLockfile(path);
          expect(lockfile.project).toEqual({
            workspaces: expect.anything(),
            catalogs: {default: {[`no-deps`]: `1.0.0`}},
          });

          await xfs.writeJsonPromise(ppath.join(path, Filename.manifest), {
            private: true,
            workspaces: [`packages/*`],
            resolutions: {[`left-pad`]: `2.0.0`, [`is-number`]: `1.0.0`},
          });

          await yarn.writeConfiguration(path, {
            catalog: {
              [`left-pad`]: `2.0.0`,
              [`no-deps`]: `1.0.0`,
            },
            packageExtensions: {
              [`left-pad@*`]: {
                dependencies: {[`no-deps`]: `1.0.0`},
              },
            },
          });

          await run(`install`, `--immutable`);

          await expect(readLockfile(path)).resolves.toEqual(lockfile);
        }),
      );

      test(
        `it should be able to install from lockfiles generated by older releases`,
        makeMonorepoEnv(async ({path, run, source}) => {
          await writeWorkspace(path, `workspace-a`, {dependencies: {[`no-deps`]: `^1.0.0`}});

          await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
            await run(`install`);
          });

          const {project, ...lockfile} = await readLockfile(path);

          await xfs.writeJsonPromise(ppath.join(path, Filename.lockfile), {
            ...lockfile,
            workspaces: project.workspaces,
          });

          await run(`install`);

          const migrated = await readLockfile(path);

          expect(migrated).not.toHaveProperty(`workspaces`);
          expect(migrated.project).toEqual(project);

          // The locked resolutions must have been preserved
          expect(migrated.entries[`no-deps@npm:^1.0.0`].resolution.resolution).toEqual(`no-deps@npm:1.0.0`);
        }),
      );
    });

    describe(`Tree hashes`, () => {
      test(
        `it should cover the whole dependency tree, unlike the hashes from the lockfile`,
        makeMonorepoEnv(async ({path, run}) => {
          await writeWorkspace(path, `workspace-a`, {dependencies: {[`one-range-dep`]: `1.0.0`}});
          await writeWorkspace(path, `workspace-b`, {dependencies: {[`workspace-a`]: `workspace:^`}});
          await writeWorkspace(path, `workspace-c`, {dependencies: {[`no-deps`]: `2.0.0`}});

          await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`, `2.0.0`])]]), async () => {
            await run(`install`);
          });

          const before = await getTreeHashes(run);
          const lockfileBefore = await readLockfile(path);

          await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`, `1.1.0`, `2.0.0`])]]), async () => {
            await run(`up`, `-R`, `no-deps`);
          });

          const after = await getTreeHashes(run);
          const lockfileAfter = await readLockfile(path);

          expect(lockfileAfter.project).toEqual(lockfileBefore.project);

          expect(after[`workspace-a`]).not.toEqual(before[`workspace-a`]);
          expect(after[`workspace-b`]).not.toEqual(before[`workspace-b`]);
          expect(after[`workspace-c`]).toEqual(before[`workspace-c`]);
        }),
      );
    });

    describe(`Changed workspaces`, () => {
      test(
        `it shouldn't report any workspace when the lockfile didn't change`,
        makeMonorepoEnv(async ({path, run}) => {
          await writeWorkspace(path, `workspace-a`, {dependencies: {[`no-deps`]: `1.0.0`}});

          await run(`install`);
          await initRepository(path);

          await expect(getChangedWorkspaces(run)).resolves.toEqual([]);
        }),
      );

      test(
        `it should only report the workspaces that depend on an updated transitive dependency`,
        makeMonorepoEnv(async ({path, run}) => {
          await writeWorkspace(path, `workspace-a`, {dependencies: {[`one-range-dep`]: `1.0.0`}});
          await writeWorkspace(path, `workspace-b`, {dependencies: {[`no-deps`]: `2.0.0`}});
          await writeWorkspace(path, `workspace-c`, {dependencies: {[`workspace-a`]: `workspace:^`}});
          await writeWorkspace(path, `workspace-d`, {dependencies: {[`workspace-b`]: `workspace:^`}});

          await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`, `2.0.0`])]]), async () => {
            await run(`install`);
          });

          await initRepository(path);

          await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`, `1.1.0`, `2.0.0`])]]), async () => {
            await run(`up`, `-R`, `no-deps`);
          });

          // workspace-c doesn't list one-range-dep itself, but it's part of
          // its dependency tree through workspace-a
          await expect(getChangedWorkspaces(run)).resolves.toEqual([
            `workspace-a`,
            `workspace-c`,
          ]);
        }),
      );

      test(
        `it should report the workspaces depending on a workspace whose dependencies changed`,
        makeMonorepoEnv(async ({path, run}) => {
          await writeWorkspace(path, `workspace-a`, {dependencies: {[`no-deps`]: `1.0.0`}});
          await writeWorkspace(path, `workspace-b`, {dependencies: {[`workspace-a`]: `workspace:^`}});
          await writeWorkspace(path, `workspace-c`, {dependencies: {[`no-deps`]: `1.0.0`}});

          await run(`install`);
          await initRepository(path);

          await writeWorkspace(path, `workspace-a`, {dependencies: {[`no-deps`]: `2.0.0`}});
          await run(`install`);

          await expect(getChangedWorkspaces(run)).resolves.toEqual([
            `workspace-a`,
            `workspace-b`,
          ]);
        }),
      );

      test(
        `it should support dependency cycles between workspaces`,
        makeMonorepoEnv(async ({path, run}) => {
          await writeWorkspace(path, `workspace-a`, {dependencies: {[`workspace-b`]: `workspace:^`}});
          await writeWorkspace(path, `workspace-b`, {dependencies: {[`workspace-a`]: `workspace:^`, [`one-range-dep`]: `1.0.0`}});
          await writeWorkspace(path, `workspace-c`, {dependencies: {[`no-deps`]: `2.0.0`}});

          await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`, `2.0.0`])]]), async () => {
            await run(`install`);
          });

          await initRepository(path);

          await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`, `1.1.0`, `2.0.0`])]]), async () => {
            await run(`up`, `-R`, `no-deps`);
          });

          await expect(getChangedWorkspaces(run)).resolves.toEqual([
            `workspace-a`,
            `workspace-b`,
          ]);
        }),
      );

      test(
        `it should only report the workspaces affected by a new dependency override`,
        makeMonorepoEnv(async ({path, run}) => {
          await writeWorkspace(path, `workspace-a`, {dependencies: {[`one-range-dep`]: `1.0.0`}});
          await writeWorkspace(path, `workspace-b`, {dependencies: {[`no-deps`]: `2.0.0`}});

          await run(`install`);
          await initRepository(path);

          await xfs.writeJsonPromise(ppath.join(path, Filename.manifest), {
            private: true,
            workspaces: [`packages/*`],
            resolutions: {[`one-range-dep/no-deps`]: `1.0.0`},
          });

          await run(`install`);

          // The root workspace is reported because its manifest changed
          await expect(getChangedWorkspaces(run)).resolves.toEqual([
            `root-workspace`,
            `workspace-a`,
          ]);
        }),
      );

      test(
        `it should only report the workspaces affected by a catalog update`,
        makeTemporaryEnv({
          private: true,
          workspaces: [`packages/*`],
          resolutions: {[`one-range-dep/no-deps`]: `catalog:`},
        }, async ({path, run}) => {
          // workspace-a references the catalog itself, workspace-b only
          // depends on a package whose dependencies are overridden by it
          await writeWorkspace(path, `workspace-a`, {dependencies: {[`no-deps`]: `catalog:`}});
          await writeWorkspace(path, `workspace-b`, {dependencies: {[`one-range-dep`]: `1.0.0`}});
          await writeWorkspace(path, `workspace-c`, {dependencies: {[`workspace-b`]: `workspace:^`}});
          await writeWorkspace(path, `workspace-d`, {dependencies: {[`no-deps`]: `2.0.0`}});

          await yarn.writeConfiguration(path, {catalog: {[`no-deps`]: `1.0.0`}});

          await run(`install`);
          await initRepository(path);

          await yarn.writeConfiguration(path, {catalog: {[`no-deps`]: `1.1.0`}});
          await run(`install`);

          const lockfile = await readLockfile(path);

          expect(lockfile.project.catalogs).toEqual({default: {[`no-deps`]: `1.1.0`}});
          expect(lockfile.project.dependencyOverrides).toEqual({[`one-range-dep/no-deps`]: `catalog:`});

          // The configuration file is located in the root workspace
          await expect(getChangedWorkspaces(run)).resolves.toEqual([
            `root-workspace`,
            `workspace-a`,
            `workspace-b`,
            `workspace-c`,
          ]);
        }),
      );

      test(
        `it should only report the workspaces affected by a new package extension`,
        makeMonorepoEnv(async ({path, run}) => {
          await writeWorkspace(path, `workspace-a`, {dependencies: {[`various-requires`]: `1.0.0`}});
          await writeWorkspace(path, `workspace-b`, {dependencies: {[`no-deps`]: `1.0.0`}});

          await run(`install`);
          await initRepository(path);

          await yarn.writeConfiguration(path, {
            packageExtensions: {
              [`various-requires@*`]: {
                dependencies: {[`no-deps`]: `1.0.0`},
              },
            },
          });

          await run(`install`);

          // The configuration file is located in the root workspace
          await expect(getChangedWorkspaces(run)).resolves.toEqual([
            `root-workspace`,
            `workspace-a`,
          ]);
        }),
      );

      test(
        `it should report the workspaces depending on a transient dependency that changed`,
        makeMonorepoEnv(async ({path, run}) => {
          const portalManifestPath = ppath.join(path, `packages/workspace-a/vendor/portal/package.json` as PortablePath);

          await writeWorkspace(path, `workspace-a`, {dependencies: {[`portal`]: `portal:./vendor/portal`}});
          await writeWorkspace(path, `workspace-b`, {dependencies: {[`workspace-a`]: `workspace:^`}});
          await writeWorkspace(path, `workspace-c`, {dependencies: {[`no-deps`]: `1.0.0`}});

          await xfs.mkdirpPromise(ppath.dirname(portalManifestPath));
          await xfs.writeJsonPromise(portalManifestPath, {
            name: `portal`,
            version: `1.0.0`,
            dependencies: {[`no-deps`]: `1.0.0`},
          });

          await run(`install`);
          await initRepository(path);

          await xfs.writeJsonPromise(portalManifestPath, {
            name: `portal`,
            version: `1.0.0`,
            dependencies: {[`no-deps`]: `2.0.0`},
          });

          await run(`install`);

          await expect(getChangedWorkspaces(run)).resolves.toEqual([
            `workspace-a`,
            `workspace-b`,
          ]);
        }),
      );

      test(
        `it shouldn't report the workspaces whose dependency tree didn't change`,
        makeMonorepoEnv(async ({path, run}) => {
          await writeWorkspace(path, `workspace-a`, {dependencies: {[`one-range-dep`]: `1.0.0`}});
          await writeWorkspace(path, `workspace-b`);

          await run(`install`);
          await initRepository(path);

          await run(`add`, `no-deps@2.0.0`, {cwd: ppath.join(path, `packages/workspace-b` as PortablePath)});

          await expect(getChangedWorkspaces(run)).resolves.toEqual([
            `workspace-b`,
          ]);
        }),
      );

      test(
        `it should compare the resolution tables of the islands as a whole`,
        makeMonorepoEnv(async ({path, run}) => {
          await writeWorkspace(path, `workspace-a`, {dependencies: {[`no-deps`]: `1.0.0`}});
          await writeWorkspace(path, `workspace-b`, {dependencies: {[`no-deps`]: `1.0.0`}});
          await writeWorkspace(path, `workspace-c`, {dependencies: {[`no-deps`]: `1.0.0`}});

          await yarn.writeConfiguration(path, {
            unstableIslands: {
              main: {
                workspaces: [`workspace-a`, `workspace-b`],
                linker: `node-modules`,
              },
            },
          });

          await run(`install`);
          await initRepository(path);

          // Changes made outside of the island don't affect its workspaces
          await writeWorkspace(path, `workspace-c`, {dependencies: {[`no-deps`]: `2.0.0`}});
          await run(`install`);

          await expect(getChangedWorkspaces(run)).resolves.toEqual([
            `workspace-c`,
          ]);

          await commit(path, `Second commit`);

          // Islands aren't walked, so all their workspaces are reported
          // as soon as something changes in their resolution table
          await writeWorkspace(path, `workspace-b`, {dependencies: {[`no-deps`]: `1.0.0`, [`one-fixed-dep`]: `1.0.0`}});
          await run(`install`);

          await expect(getChangedWorkspaces(run)).resolves.toEqual([
            `workspace-a`,
            `workspace-b`,
          ]);
        }),
      );

      test(
        `it should report all workspaces when the base lockfile was generated by an older release`,
        makeMonorepoEnv(async ({path, run}) => {
          await writeWorkspace(path, `workspace-a`, {dependencies: {[`no-deps`]: `1.0.0`}});
          await writeWorkspace(path, `workspace-b`);

          await run(`install`);

          const {project, ...lockfile} = await readLockfile(path);

          await xfs.writeJsonPromise(ppath.join(path, Filename.lockfile), {
            ...lockfile,
            workspaces: project.workspaces,
          });

          await initRepository(path);
          await run(`install`);

          // The hashes from the base lockfile have different semantics, so we
          // can't tell what changed; better be safe than sorry
          await expect(getChangedWorkspaces(run)).resolves.toEqual([
            `root-workspace`,
            `workspace-a`,
            `workspace-b`,
          ]);
        }),
      );
    });
  });
});
