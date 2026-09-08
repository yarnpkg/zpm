import {PortablePath, npath, ppath, xfs} from '@yarnpkg/fslib';
import http, {RequestListener}           from 'http';
import {exec, fs, tests, yarn}           from 'pkg-tests-core';

type HashRunOptions = {
  enableWorkspaceHashes?: boolean;
  env?: Record<string, string | undefined>;
  unsafeHttpWhitelist?: Array<string>;
};

async function readLockfile(path: PortablePath) {
  const raw = await xfs.readFilePromise(`${path}/yarn.lock` as PortablePath, `utf8`);
  return JSON.parse(raw);
}

async function readTreeHashes(run: tests.Run, enableWorkspaceHashes?: boolean, options?: HashRunOptions) {
  const callOptions: HashRunOptions = {...options};
  if (typeof enableWorkspaceHashes === `boolean`)
    callOptions.enableWorkspaceHashes = enableWorkspaceHashes;

  const {stdout} = await run(`workspaces`, `list`, `--json`, `--tree-hash`, callOptions);
  const workspaces = stdout.trim().split(`\n`).map(line => JSON.parse(line));

  for (const workspace of workspaces)
    expect(workspace.treeHash).toMatch(/^[0-9a-f]+$/);

  return Object.fromEntries(workspaces.map(({location, name, treeHash}) => [
    name ?? (location === `.` ? `root-workspace` : ppath.basename(location as PortablePath)),
    treeHash,
  ]));
}

const forEachVerboseDone = tests.FEATURE_CHECKS.forEachVerboseDone
  ? []
  : [`Done\n`];

const gitInit = async (path: PortablePath, message = `Initial commit`) => {
  const git = (...args: Array<string>) => exec.execFile(`git`, args, {cwd: path});
  await exec.execGitInit({cwd: path});
  await git(`add`, `-A`);
  await git(`commit`, `-m`, message);
  return git;
};

const expectForEachSince = async (
  run: tests.Run,
  expectedWorkspaces: Array<string>,
  options: HashRunOptions = {},
) => {
  const expectedStdout = [...expectedWorkspaces.map(name => `${name}\n`), ...forEachVerboseDone].join(``);
  await expect(run(`workspaces`, `foreach`, `--since`, `run`, `print`, options)).resolves.toEqual({
    code: 0,
    stderr: ``,
    stdout: expectedStdout,
  });
};

const startServer = async (listener: RequestListener) => {
  const server = http.createServer(listener);
  server.unref();

  await new Promise<void>((resolve, reject) => {
    server.once(`error`, reject);
    server.listen(0, `127.0.0.1`, resolve);
  });

  return {
    close: () => new Promise<void>((resolve, reject) => {
      server.close(error => error ? reject(error) : resolve());
    }),
    url: `http://127.0.0.1:${(server.address() as any).port}`,
  };
};

const NO_DEPS_PATCH = `diff --git a/index.js b/index.js
--- a/index.js
+++ b/index.js
@@ -1,1 +1,2 @@
 module.exports = require(\`./package.json\`);
+module.exports.hello = \`before\`;
`;

const NO_DEPS_MANIFEST_PATCH = `diff --git a/package.json b/package.json
--- a/package.json
+++ b/package.json
@@ -1,4 +1,7 @@
 {
     "name": "no-deps",
-    "version": "1.0.0"
+    "version": "1.0.0",
+    "dependencies": {
+        "is-number": "1.0.0"
+    }
 }
`;

// A monorepo whose workspace-a depends on a registry package and
// workspace-b depends on workspace-a, so each workspace has a
// different dependency tree to hash.
const makeHashesEnv = (fn: tests.RunFunction) => makeTemporaryMonorepoEnv(
  {
    private: true,
    workspaces: [`packages/*`],
    scripts: {print: `echo Root Workspace`},
  },
  {
    [`packages/workspace-a`]: {
      name: `workspace-a`,
      version: `1.0.0`,
      scripts: {print: `echo Test Workspace A`},
      dependencies: {
        [`no-deps`]: `1.0.0`,
      },
    },
    [`packages/workspace-b`]: {
      name: `workspace-b`,
      version: `1.0.0`,
      scripts: {print: `echo Test Workspace B`},
      dependencies: {
        [`workspace-a`]: `workspace:*`,
      },
    },
  },
  fn,
);

describe(`Features`, () => {
  describe(`Workspace hashes`, () => {
    test(
      `the lockfile stores one dependency tree hash per workspace by default, matching on-demand hashes`,
      makeHashesEnv(async ({path, run}) => {
        await run(`install`);

        const stored = await readTreeHashes(run);
        const lockfile = await readLockfile(path);

        expect(Object.keys(lockfile.workspaces ?? {}).sort()).toEqual([
          `root-workspace`,
          `workspace-a`,
          `workspace-b`,
        ]);
        expect(lockfile.workspaces).toEqual(stored);

        // The query uses stored hashes whenever present, regardless of the flag.
        // Remove them through install before testing on-demand computation.
        await run(`install`, {enableWorkspaceHashes: false});
        expect(`workspaces` in await readLockfile(path)).toBe(false);
        const onDemand = await readTreeHashes(run, false);
        expect(onDemand).toEqual(stored);
      }),
    );

    test(
      `the workspaces section is omitted when enableWorkspaceHashes is false, and the lockfile without it still parses`,
      makeHashesEnv(async ({path, run}) => {
        await run(`install`, {enableWorkspaceHashes: false});

        const lockfile = await readLockfile(path);
        expect(`workspaces` in lockfile).toBe(false);

        // The lockfile without the section still parses and the
        // install stays fresh rather than looping.
        const second = await run(`install`, {enableWorkspaceHashes: false});
        expect(second.stdout).toContain(`up-to-date`);
      }),
    );

    test(
      `a dependency-free on-demand tree hash needs no immutable cache folder`,
      makeTemporaryEnv({name: `root-workspace`, private: true}, async ({path, run}) => {
        await run(`install`, {enableWorkspaceHashes: false});
        const cacheFolder = `${path}/missing-cache` as PortablePath;
        expect(await xfs.existsPromise(cacheFolder)).toBe(false);

        const {stdout} = await run(`workspaces`, `list`, `--json`, `--tree-hash`, {
          enableWorkspaceHashes: false,
          enableGlobalCache: false,
          enableImmutableCache: true,
          cacheFolder,
        });
        expect(JSON.parse(stdout)).toMatchObject({name: `root-workspace`, treeHash: expect.stringMatching(/^[0-9a-f]+$/)});
        expect(await xfs.existsPromise(cacheFolder)).toBe(false);
      }),
    );

    test(
      `a cold file: folder tree hash populates a missing mutable local cache`,
      makeTemporaryEnv({
        name: `root-workspace`,
        dependencies: {[`no-deps`]: `file:./vendor/no-deps`},
      }, async ({path, run}) => {
        await xfs.copyPromise(`${path}/vendor/no-deps` as PortablePath, npath.toPortablePath(await tests.getPackageDirectoryPath(`no-deps`, `1.0.0`)));
        await run(`install`, {enableWorkspaceHashes: true});
        const expected = await readTreeHashes(run, true);
        await run(`install`, {enableWorkspaceHashes: false});
        await xfs.removePromise(`${path}/.yarn/cache` as PortablePath);
        await xfs.removePromise(`${path}/.yarn/global` as PortablePath);
        const cacheFolder = `${path}/missing-parent/cache` as PortablePath;
        expect(await xfs.existsPromise(cacheFolder)).toBe(false);

        const {stdout} = await run(`workspaces`, `list`, `--json`, `--tree-hash`, {
          enableWorkspaceHashes: false,
          enableGlobalCache: false,
          enableImmutableCache: false,
          cacheFolder,
        });
        expect(JSON.parse(stdout).treeHash).toBe(expected[`root-workspace`]);
        expect((await xfs.readdirPromise(cacheFolder)).some(name => name.endsWith(`.zip`))).toBe(true);
      }),
    );

    test(
      `--since still attributes lockfile changes to the affected workspaces when enableWorkspaceHashes is false`,
      makeTemporaryMonorepoEnv(
        {
          private: true,
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            scripts: {print: `echo Test Workspace A`},
            dependencies: {[`one-range-dep`]: `1.0.0`},
          },
          [`packages/workspace-b`]: {
            name: `workspace-b`,
            version: `1.0.0`,
            scripts: {print: `echo Test Workspace B`},
          },
        },
        async ({path, run}) => {
          // Install with only no-deps@1.0.0 visible, so one-range-dep resolves to no-deps@1.0.0.
          await tests.setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
            await run(`install`, {enableWorkspaceHashes: false});
          });

          await gitInit(path, `First commit`);

          // Now make no-deps@1.1.0 visible and upgrade; only the lockfile changes,
          // and only workspace-a's dependency tree changed through it.
          await tests.setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`, `1.1.0`])]]), async () => {
            await run(`up`, `-R`, `no-deps`, {enableWorkspaceHashes: false});
          });

          await expectForEachSince(run, [`Test Workspace A`], {enableWorkspaceHashes: false});
        },
      ),
    );

    test(
      `--since toggling hashes via environment overrides selects nothing across commits`,
      makeHashesEnv(async ({path, run}) => {
        await run(`install`, {enableWorkspaceHashes: true});
        const stored = await readTreeHashes(run, true);
        const git = await gitInit(path, `Hashes on`);

        for (const enableWorkspaceHashes of [false, true]) {
          await run(`install`, {enableWorkspaceHashes});
          const lockfile = await readLockfile(path);
          if (enableWorkspaceHashes)
            expect(lockfile.workspaces).toEqual(stored);
          else
            expect(`workspaces` in lockfile).toBe(false);

          expect(await readTreeHashes(run, enableWorkspaceHashes)).toEqual(stored);
          await expectForEachSince(run, [], {enableWorkspaceHashes});

          await git(`add`, `-A`);
          await git(`commit`, `-m`, `Hashes ${enableWorkspaceHashes ? `on` : `off`}`);
        }

        // A positive control prevents an empty-output assertion from passing
        // just because the fixture has no runnable workspace scripts.
        await xfs.writeFilePromise(`${path}/packages/workspace-a/README.md` as PortablePath, `Changed\n`);
        await expectForEachSince(run, [`Test Workspace A`], {enableWorkspaceHashes: true});
      }),
    );

    for (const {name, manifest, rootManifest, configuration} of [
      {
        name: `default catalog`,
        manifest: {dependencies: {[`no-deps`]: `catalog:`}},
        rootManifest: {},
        configuration: {catalog: {[`no-deps`]: `2.0.0`}},
      },
      {
        name: `named catalog`,
        manifest: {dependencies: {[`no-deps`]: `catalog:runtime`}},
        rootManifest: {},
        configuration: {catalogs: {runtime: {[`no-deps`]: `2.0.0`}}},
      },
      {
        name: `root resolutions`,
        manifest: {dependencies: {[`no-deps`]: `1.0.0`}},
        rootManifest: {resolutions: {[`no-deps`]: `2.0.0`}},
        configuration: {},
      },
      {
        name: `inherited profile devDependencies`,
        manifest: {extends: [`application`]},
        rootManifest: {},
        configuration: {
          workspaceProfiles: {
            base: {devDependencies: {[`no-deps`]: `1.0.0`}},
            application: {extends: [`base`]},
          },
        },
      },
      {
        name: `package extension dependencies`,
        manifest: {dependencies: {[`no-deps`]: `2.0.0`}},
        rootManifest: {},
        configuration: {
          packageExtensions: {
            [`no-deps@2.0.0`]: {dependencies: {[`one-range-dep`]: `1.0.0`}},
          },
        },
      },
    ]) {
      test(
        `${name}: stored and on-demand tree hashes match`,
        makeTemporaryMonorepoEnv(
          {
            name: `root-workspace`,
            private: true,
            workspaces: [`packages/*`],
            ...rootManifest,
          },
          {
            [`packages/workspace-a`]: {
              name: `workspace-a`,
              version: `1.0.0`,
              ...manifest,
            },
            [`packages/workspace-b`]: {
              name: `workspace-b`,
              version: `1.0.0`,
              dependencies: {[`workspace-a`]: `workspace:*`},
            },
          },
          async ({path, run}) => {
            await yarn.writeConfiguration(path, configuration);
            await run(`install`, {enableWorkspaceHashes: true});
            const stored = await readTreeHashes(run, true);
            expect(Object.keys(stored).sort()).toEqual([`root-workspace`, `workspace-a`, `workspace-b`]);
            expect((await readLockfile(path)).workspaces).toEqual(stored);

            await run(`install`, {enableWorkspaceHashes: false});
            expect(`workspaces` in await readLockfile(path)).toBe(false);
            const onDemand = await readTreeHashes(run, false);
            expect(onDemand).toEqual(stored);
          },
        ),
      );
    }

    for (const {name, dependencies, patch} of [
      {name: `user patch`, dependencies: {[`no-deps`]: `patch:no-deps@1.0.0#~/no-deps.patch`}, patch: NO_DEPS_PATCH},
      {name: `package.json-changing user patch`, dependencies: {[`no-deps`]: `patch:no-deps@1.0.0#~/no-deps.patch`}, patch: NO_DEPS_MANIFEST_PATCH},
      {name: `builtin patch`, dependencies: {resolve: `1.9.0`}, patch: null},
    ]) {
      test(
        `${name}: stored and on-demand tree hashes match`,
        makeTemporaryMonorepoEnv(
          {name: `root-workspace`, private: true, workspaces: [`packages/*`]},
          {
            [`packages/workspace-a`]: {name: `workspace-a`, dependencies},
            [`packages/workspace-b`]: {name: `workspace-b`, dependencies: {[`workspace-a`]: `workspace:*`}},
          },
          async ({path, run, source}) => {
            if (patch !== null)
              await xfs.writeFilePromise(`${path}/no-deps.patch` as PortablePath, patch);

            await run(`install`, {enableWorkspaceHashes: true});
            const lockfile = await readLockfile(path);
            const stored = await readTreeHashes(run, true);
            expect(Object.keys(stored).sort()).toEqual([`root-workspace`, `workspace-a`, `workspace-b`]);
            expect(lockfile.workspaces).toEqual(stored);

            // Prove the patch was applied, including resolve@1.9.0's builtin patch.
            if (patch === null)
              await expect(source(`require('resolve/lib/normalize-options').toString()`, {cwd: `${path}/packages/workspace-a`})).resolves.toContain(`forceNodeResolution`);
            else if (patch === NO_DEPS_MANIFEST_PATCH)
              await expect(source(`require('no-deps').dependencies['is-number']`, {cwd: `${path}/packages/workspace-a`})).resolves.toMatchObject({version: `1.0.0`});
            else
              await expect(source(`require('no-deps').hello`, {cwd: `${path}/packages/workspace-a`})).resolves.toBe(`before`);

            for (const enableWorkspaceHashes of [false, true]) {
              await run(`install`, {enableWorkspaceHashes});
              expect(`workspaces` in await readLockfile(path)).toBe(enableWorkspaceHashes);
              expect(await readTreeHashes(run, enableWorkspaceHashes)).toEqual(stored);
            }
          },
        ),
      );
    }

    test(
      `--since keeps attributing unaffected workspaces when a historical workspace manifest went missing`,
      makeTemporaryMonorepoEnv(
        {
          private: true,
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            scripts: {print: `echo Test Workspace A`},
            dependencies: {[`workspace-c`]: `workspace:*`},
          },
          [`packages/workspace-b`]: {
            name: `workspace-b`,
            version: `1.0.0`,
            scripts: {print: `echo Test Workspace B`},
            dependencies: {[`one-range-dep`]: `1.0.0`},
          },
          [`packages/workspace-c`]: {
            name: `workspace-c`,
            version: `1.0.0`,
            scripts: {print: `echo Test Workspace C`},
          },
        },
        async ({path, run}) => {
          // Install with only no-deps@1.0.0 visible, so one-range-dep resolves to no-deps@1.0.0.
          await tests.setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
            await run(`install`, {enableWorkspaceHashes: false});
          });

          const git = await gitInit(path, `First commit`);

          // The old-side walk must find workspace-c at its historical path.
          await git(`mv`, `${path}/packages/workspace-c`, `${path}/packages/workspace-c-renamed`);

          // Make no-deps@1.1.0 visible and upgrade; only the lockfile
          // changes, and only workspace-b's dependency tree changed through it.
          await tests.setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`, `1.1.0`])]]), async () => {
            await run(`up`, `-R`, `no-deps`, {enableWorkspaceHashes: false});
          });

          // B's tree changed, A's didn't, and C runs because its manifest moved.
          await expectForEachSince(run, [`Test Workspace B`, `Test Workspace C`], {enableWorkspaceHashes: false});
        },
      ),
    );

    test(
      `--since retains direct file attribution against a legacy Berry lockfile`,
      makeTemporaryEnv({
        name: `root-workspace`,
        private: true,
        scripts: {print: `echo Root Workspace`},
        dependencies: {[`no-deps`]: `1.0.0`},
      }, async ({path, run}) => {
        await xfs.writeFilePromise(`${path}/yarn.lock` as PortablePath, [
          `# This file is generated by running "yarn install" inside your project.`,
          `# Manual changes might be lost - proceed with caution!`,
          ``,
          `__metadata:`,
          `  version: 8`,
          `  cacheKey: 0c0`,
          `"no-deps@npm:1.0.0":`,
          `  version: 1.0.0`,
          `  resolution: "no-deps@npm:1.0.0"`,
          `  languageName: node`,
          `  linkType: hard`,
          ``,
        ].join(`\n`));
        await gitInit(path, `Legacy Berry install`);

        const manifest = await yarn.readManifest(path);
        delete manifest.dependencies;
        await yarn.writeManifest(path, manifest);
        await run(`install`);
        await expectForEachSince(run, [`Root Workspace`]);
      }),
    );

    for (const historicalLockfile of [``, `{invalid native lockfile`]) {
      test(
        `--since retains direct attribution with ${historicalLockfile ? `invalid native` : `empty`} historical lockfile contents`,
        makeHashesEnv(async ({path, run}) => {
          await run(`install`, {enableWorkspaceHashes: false});
          const lockfilePath = `${path}/yarn.lock` as PortablePath;
          const currentLockfile = await xfs.readFilePromise(lockfilePath, `utf8`);
          await xfs.writeFilePromise(lockfilePath, historicalLockfile);
          await gitInit(path, `Unavailable historical graph`);
          await xfs.writeFilePromise(lockfilePath, currentLockfile);

          await expectForEachSince(run, [], {enableWorkspaceHashes: false});
          await xfs.writeFilePromise(`${path}/packages/workspace-a/README.md` as PortablePath, `Changed\n`);
          await expectForEachSince(run, [`Test Workspace A`], {enableWorkspaceHashes: false});
        }),
      );
    }

    test(
      `--since propagates current hash errors rather than using historical fallback`,
      makeHashesEnv(async ({path, run}) => {
        await run(`install`, {enableWorkspaceHashes: false});
        await gitInit(path, `Readable historical graph`);
        const lockfile = await readLockfile(path);
        lockfile.entries = {};
        await fs.writeJson(`${path}/yarn.lock` as PortablePath, lockfile);

        await expect(run(`debug`, `print-changed-workspaces`, `--since`, `HEAD`, {enableWorkspaceHashes: false})).rejects.toMatchObject({
          code: 1,
          stdout: expect.stringContaining(`Missing resolution for descriptor`),
        });
      }),
    );

    test(
      `stored and historical hashes use the same fallback names for unnamed workspaces`,
      makeTemporaryMonorepoEnv(
        {private: true, workspaces: [`packages/*`], scripts: {print: `echo Root Workspace`}},
        {
          [`packages/anonymous`]: {version: `1.0.0`, scripts: {print: `echo Anonymous Workspace`}},
          [`packages/control`]: {name: `control`, scripts: {print: `echo Control Workspace`}},
        },
        async ({path, run}) => {
          await run(`install`, {enableWorkspaceHashes: true});
          const stored = await readTreeHashes(run, true);
          expect(Object.keys(stored).sort()).toEqual([`anonymous`, `control`, `root-workspace`]);
          expect((await readLockfile(path)).workspaces).toEqual(stored);

          await run(`install`, {enableWorkspaceHashes: false});
          expect(`workspaces` in await readLockfile(path)).toBe(false);
          expect(await readTreeHashes(run, false)).toEqual(stored);
          await gitInit(path, `Unnamed workspaces without stored hashes`);

          // Query directly so a lazy install cannot erase the edit that forces replay.
          await xfs.appendFilePromise(`${path}/yarn.lock` as PortablePath, `\n`);
          await expect(run(`debug`, `print-changed-workspaces`, `--since`, `HEAD`, {enableWorkspaceHashes: false})).resolves.toEqual({
            code: 0,
            stderr: ``,
            stdout: ``,
          });
          await xfs.writeFilePromise(`${path}/packages/anonymous/README.md` as PortablePath, `Changed\n`);
          await expectForEachSince(run, [`Anonymous Workspace`], {enableWorkspaceHashes: false});
        },
      ),
    );

    for (const {edit, initialHashes, initialConfiguration, modifiedConfiguration, storesHashesAfter} of [
      {
        edit: `comment-only`,
        initialHashes: true,
        initialConfiguration: `enableWorkspaceHashes: true\n`,
        modifiedConfiguration: `enableWorkspaceHashes: true\n# No setting changed\n`,
        storesHashesAfter: true,
      },
      {
        edit: `whitespace-only`,
        initialHashes: false,
        initialConfiguration: `enableWorkspaceHashes: false\n`,
        modifiedConfiguration: `enableWorkspaceHashes:   false\n\n`,
        storesHashesAfter: false,
      },
      {
        edit: `persisted toggle from true to false`,
        initialHashes: true,
        initialConfiguration: `enableWorkspaceHashes: true\n`,
        modifiedConfiguration: `enableWorkspaceHashes: false\n`,
        storesHashesAfter: false,
      },
      {
        edit: `persisted toggle from false to true`,
        initialHashes: false,
        initialConfiguration: `enableWorkspaceHashes: false\n`,
        modifiedConfiguration: `enableWorkspaceHashes: true\n`,
        storesHashesAfter: true,
      },
    ]) {
      test(
        `--since marks all workspaces changed for a ${edit} in .yarnrc.yml`,
        makeTemporaryMonorepoEnv(
          {
            name: `root-workspace`,
            private: true,
            workspaces: [`packages/*`],
            scripts: {print: `echo Root Workspace`},
          },
          {
            [`packages/workspace-a`]: {name: `workspace-a`, scripts: {print: `echo Test Workspace A`}},
            [`packages/workspace-b`]: {name: `workspace-b`, scripts: {print: `echo Test Workspace B`}},
          },
          async ({path, run}) => {
            const configPath = `${path}/.yarnrc.yml` as PortablePath;
            await xfs.writeFilePromise(configPath, initialConfiguration);
            await run(`install`);
            expect(`workspaces` in await readLockfile(path)).toBe(initialHashes);
            const {stdout: initialHashesStdout} = await run(`workspaces`, `list`, `--json`, `--tree-hash`);

            await gitInit(path, `Initial configuration`);
            await expectForEachSince(run, []);

            await xfs.writeFilePromise(configPath, modifiedConfiguration);
            await run(`install`);
            expect(`workspaces` in await readLockfile(path)).toBe(storesHashesAfter);

            // Dependency trees are unchanged, but configuration edits conservatively invalidate all workspaces.
            await expect(run(`workspaces`, `list`, `--json`, `--tree-hash`)).resolves.toMatchObject({stdout: initialHashesStdout});
            await expectForEachSince(run, [`Root Workspace`, `Test Workspace A`, `Test Workspace B`]);

            // Restoring the configuration returns to clean selection.
            await xfs.writeFilePromise(configPath, initialConfiguration);
            await run(`install`);
            await expectForEachSince(run, []);
          },
        ),
      );
    }

    for (const enableWorkspaceHashes of [true, false]) {
      for (const conversion of [`CRLF`, `smudge filter`]) {
        test(
          `--since treats ${conversion} checkout conversion as unchanged configuration (hashes ${enableWorkspaceHashes})`,
          makeTemporaryMonorepoEnv(
            {
              name: `root-workspace`,
              private: true,
              workspaces: [`packages/*`],
              scripts: {print: `echo Root Workspace`},
            },
            {
              [`packages/workspace-a`]: {name: `workspace-a`, scripts: {print: `echo Test Workspace A`}},
              [`packages/workspace-b`]: {name: `workspace-b`, scripts: {print: `echo Test Workspace B`}},
            },
            async ({path, run}) => {
              const configPath = `${path}/.yarnrc.yml` as PortablePath;
              const initialConfiguration = `enableWorkspaceHashes: ${enableWorkspaceHashes}\n`;
              await xfs.writeFilePromise(configPath, initialConfiguration);
              await run(`install`);
              expect(`workspaces` in await readLockfile(path)).toBe(enableWorkspaceHashes);

              const git = await gitInit(path, `Configuration before checkout conversion`);
              await git(`config`, `core.autocrlf`, String(conversion === `CRLF`));
              if (conversion === `smudge filter`) {
                await xfs.writeFilePromise(`${path}/.gitattributes` as PortablePath, `.yarnrc.yml filter=workspace-hashes text eol=lf\n`);
                await git(`config`, `filter.workspace-hashes.clean`, `sed '/^# Git checkout conversion$/d'`);
                await git(`config`, `filter.workspace-hashes.smudge`, `cat && printf '# Git checkout conversion\\n'`);
                await git(`config`, `filter.workspace-hashes.required`, `true`);
                await git(`add`, `.gitattributes`);
                await git(`commit`, `-m`, `Configure checkout filter`);
              }

              await xfs.removePromise(configPath);
              await git(`checkout`, `HEAD`, `--`, `.yarnrc.yml`);
              const checkedOutConfiguration = await xfs.readFilePromise(configPath, `utf8`);
              expect(checkedOutConfiguration).not.toEqual(initialConfiguration);
              await expect(git(`show`, `HEAD:.yarnrc.yml`)).resolves.toMatchObject({stdout: initialConfiguration});
              await expect(git(`diff`, `HEAD`, `--`, `.yarnrc.yml`)).resolves.toMatchObject({stdout: ``});

              // Force comparison without a real config edit. The unrelated
              // control and root must not be selected just because Git converted it.
              await xfs.writeFilePromise(`${path}/packages/workspace-a/README.md` as PortablePath, `Changed\n`);
              await expectForEachSince(run, [`Test Workspace A`]);

              // Real configuration edits must still invalidate every workspace.
              await xfs.writeFilePromise(configPath, `${checkedOutConfiguration}# Actual configuration edit\n`);
              await expectForEachSince(run, [`Root Workspace`, `Test Workspace A`, `Test Workspace B`]);
            },
          ),
        );
      }
    }

    test(
      `--since retains an isolated user rc workspace profile during historical replay`,
      makeTemporaryMonorepoEnv(
        {private: true, workspaces: [`packages/*`]},
        {
          [`packages/workspace-a`]: {name: `workspace-a`, extends: [`shared`], scripts: {print: `echo Test Workspace A`}},
          [`packages/workspace-b`]: {name: `workspace-b`, scripts: {print: `echo Test Workspace B`}},
        },
        async ({path, run, source}) => {
          await xfs.mktempPromise(async homePath => {
            const env = {HOME: homePath, USERPROFILE: homePath};
            const configuration = {enableWorkspaceHashes: false, env};
            await yarn.writeConfiguration(homePath, {
              workspaceProfiles: {shared: {devDependencies: {[`workspace-b`]: `workspace:*`}}},
            });
            await xfs.writeFilePromise(`${path}/README.md` as PortablePath, `Before\n`);
            await run(`install`, configuration);
            await expect(source(`require('workspace-b/package.json').name`, {cwd: `${path}/packages/workspace-a`, ...configuration})).resolves.toBe(`workspace-b`);
            expect(`workspaces` in await readLockfile(path)).toBe(false);
            await gitInit(path, `User profile outside repository`);

            await xfs.writeFilePromise(`${path}/README.md` as PortablePath, `After\n`);
            // Force a historical comparison without changing either dependency graph.
            await xfs.appendFilePromise(`${path}/yarn.lock` as PortablePath, `\n`);
            await expectForEachSince(run, [], configuration);
          });
        },
      ),
    );

    test(
      `--since preserves environment precedence over user transparent-workspace settings when toggling hashes via environment overrides`,
      makeTemporaryMonorepoEnv(
        {name: `root-workspace`, private: true, workspaces: [`packages/*`]},
        {
          [`packages/workspace-a`]: {name: `workspace-a`, dependencies: {[`no-deps`]: `1.0.0`}, scripts: {print: `echo Test Workspace A`}},
          [`packages/no-deps`]: {name: `no-deps`, version: `1.0.0`, local: true, scripts: {print: `echo Local no-deps`}},
        },
        async ({path, run, source}) => {
          await xfs.mktempPromise(async homePath => {
            const env = {HOME: homePath, USERPROFILE: homePath, YARN_ENABLE_TRANSPARENT_WORKSPACES: `false`};
            await yarn.writeConfiguration(homePath, {enableTransparentWorkspaces: true});
            await run(`install`, {enableWorkspaceHashes: true, env});
            await expect(source(`require('no-deps/package.json')`, {cwd: `${path}/packages/workspace-a`, env})).resolves.toEqual({name: `no-deps`, version: `1.0.0`});
            const stored = await readTreeHashes(run, true, {env});
            const git = await gitInit(path, `Environment overrides user settings`);

            for (const enableWorkspaceHashes of [false, true]) {
              await run(`install`, {enableWorkspaceHashes, env});
              expect(`workspaces` in await readLockfile(path)).toBe(enableWorkspaceHashes);
              expect(await readTreeHashes(run, enableWorkspaceHashes, {env})).toEqual(stored);
              await expectForEachSince(run, [], {enableWorkspaceHashes, env});
              await git(`add`, `-A`);
              await git(`commit`, `-m`, `Hashes ${enableWorkspaceHashes}`);
            }
          });
        },
      ),
    );

    for (const environmentSource of [`process`, `dotenv`]) {
      test(
        `--since reuses live configuration trust during historical replay (${environmentSource})`,
        makeTemporaryMonorepoEnv(
          {private: true, workspaces: [`packages/*`]},
          {
            [`packages/workspace-a`]: {name: `workspace-a`, scripts: {print: `echo Test Workspace A`}},
            [`packages/workspace-b`]: {name: `workspace-b`, scripts: {print: `echo Test Workspace B`}},
          },
          async ({path, runSwitch}) => {
            const env = environmentSource === `process` ? {CONFIG_INIT_SCOPE: `acme`} : {};
            await yarn.writeConfiguration(path, {
              enableWorkspaceHashes: false,
              initScope: `\${CONFIG_INIT_SCOPE}`,
              ...(environmentSource === `dotenv` ? {injectEnvironmentFiles: [`.env.local`]} : {}),
            });
            if (environmentSource === `dotenv`) {
              // This required file is available to the live project, not the checkout.
              await xfs.writeFilePromise(`${path}/.env.local` as PortablePath, `CONFIG_INIT_SCOPE=acme\n`);
              await xfs.appendFilePromise(`${path}/.gitignore` as PortablePath, `\n.env.local\n`);
            }
            await runSwitch(`switch`, `trust`, `--set`, `true`, path);
            await runSwitch(`install`, {env});
            expect(`workspaces` in await readLockfile(path)).toBe(false);

            await gitInit(path, `Trusted live configuration`);
            await xfs.writeFilePromise(`${path}/packages/workspace-a/README.md` as PortablePath, `Changed\n`);

            // Trust is scoped to the live project. The snapshot must not need its
            // own trust decision or silently fall back to selecting workspace-b.
            const forEachOptions = {env};
            const expectedStdout = [`Test Workspace A\n`, ...forEachVerboseDone].join(``);
            await expect(runSwitch(`workspaces`, `foreach`, `--since`, `run`, `print`, forEachOptions)).resolves.toEqual({
              code: 0,
              stderr: ``,
              stdout: expectedStdout,
            });

            // Reusing the configuration must not bypass its normal trust check.
            await runSwitch(`switch`, `trust`, `--set`, `false`, path);
            await expect(runSwitch(`workspaces`, `foreach`, `--since`, `run`, `print`, {env})).rejects.toMatchObject({
              code: 1,
              stdout: expect.stringContaining(`isn't trusted, so Yarn won't interpolate project configuration`),
            });
          },
        ),
      );
    }

    for (const versionFolder of [`.yarn/versions`, `.release-decisions`]) {
      test(
        `version check preserves real changes after historical replay fails (${versionFolder})`,
        makeTemporaryMonorepoEnv(
          {name: `root-workspace`, private: true, workspaces: [`packages/*`]},
          {
            [`packages/workspace-a`]: {name: `workspace-a`, version: `1.0.0`},
            [`packages/workspace-b`]: {name: `workspace-b`, version: `1.0.0`},
          },
          async ({path, run}) => {
            const configuration = {enableWorkspaceHashes: false, deferredVersionFolder: `${path}/${versionFolder}`};
            const sourcePath = `${path}/vendor/no-deps` as PortablePath;
            await xfs.copyPromise(sourcePath, npath.toPortablePath(await tests.getPackageDirectoryPath(`no-deps`, `1.0.0`)));
            const workspacePath = `${path}/packages/workspace-a` as PortablePath;
            const manifest = await yarn.readManifest(workspacePath);
            manifest.dependencies = {[`no-deps`]: `file:${sourcePath}`};
            await yarn.writeManifest(workspacePath, manifest);
            await run(`install`, configuration);

            const git = await gitInit(path, `Absolute source requires conservative historical fallback`);

            const changedPath = `${workspacePath}/README.md` as PortablePath;
            await xfs.writeFilePromise(changedPath, `Changed without a version decision\n`);
            await expect(run(`version`, `check`, configuration)).rejects.toMatchObject({
              code: 1,
              stdout: expect.stringContaining(`Couldn't auto-upgrade range * (in workspace-a)`),
            });

            const decisionPath = `${path}/${versionFolder}/unrelated.json` as PortablePath;
            await xfs.mkdirpPromise(ppath.dirname(decisionPath));
            await xfs.writeJsonPromise(decisionPath, {});
            await git(`add`, `-f`, decisionPath);

            // A bookkeeping file sorts before the source change. It must not
            // replace the real evidence and make version check pass.
            await expect(run(`version`, `check`, configuration)).rejects.toMatchObject({
              code: 1,
              stdout: expect.stringContaining(`Couldn't auto-upgrade range * (in workspace-a)`),
            });

            // Bookkeeping-only changes still don't require version decisions,
            // even when an absolute source prevents historical replay.
            await xfs.removePromise(changedPath);
            await expect(run(`version`, `check`, configuration)).resolves.toMatchObject({code: 0});
          },
        ),
      );
    }

    test(
      `--since marks all workspaces changed when a historical source escapes the snapshot`,
      makeTemporaryMonorepoEnv(
        {
          name: `root-workspace`,
          private: true,
          workspaces: [`packages/*`],
          scripts: {print: `echo Root Workspace`},
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            dependencies: {[`no-deps`]: `file:../../linked`},
            scripts: {print: `echo Test Workspace A`},
          },
          [`packages/workspace-b`]: {
            name: `workspace-b`,
            scripts: {print: `echo Test Workspace B`},
          },
        },
        async ({path, run, source}) => {
          await xfs.mktempPromise(async outsidePath => {
            await xfs.copyPromise(outsidePath, npath.toPortablePath(await tests.getPackageDirectoryPath(`no-deps`, `1.0.0`)));
            await xfs.symlinkPromise(ppath.relative(path, outsidePath), `${path}/linked` as PortablePath);
            await run(`install`, {enableWorkspaceHashes: false});
            await expect(source(`require('no-deps').version`, {cwd: `${path}/packages/workspace-a`, enableWorkspaceHashes: false})).resolves.toBe(`1.0.0`);
            expect(`workspaces` in await readLockfile(path)).toBe(false);

            await gitInit(path, `Relative symlink to an external source`);

            // The source exists for the live project, but is not part of Git history.
            // Because the historical source cannot be checked out, all workspaces conservatively change,
            // including untouched workspace-b.
            await xfs.writeFilePromise(`${path}/README.md` as PortablePath, `Trigger historical comparison\n`);
            await expectForEachSince(run, [`Root Workspace`, `Test Workspace A`, `Test Workspace B`], {enableWorkspaceHashes: false});
          });
        },
      ),
    );

    test(
      `on-demand tree hashes reuse a cached exec artifact and prepare a missing one on demand`,
      makeTemporaryEnv({
        name: `root-workspace`,
        dependencies: {[`dynamic-pkg`]: `exec:./genpkg.js`},
      }, {enableScripts: true, enableGlobalCache: false}, async ({path, run, source}) => {
        const markerPath = `${path}/generator-ran` as PortablePath;
        await xfs.writeFilePromise(`${path}/genpkg.js` as PortablePath, `
          fs.writeFileSync(path.join(process.cwd(), 'generator-ran'), 'ran');
          fs.writeFileSync(path.join(execEnv.buildDir, 'index.js'), 'module.exports = 42;');
          fs.writeFileSync(path.join(execEnv.buildDir, 'package.json'), '{}');
        `);
        await run(`install`, {enableWorkspaceHashes: true});
        await expect(source(`require('dynamic-pkg')`)).resolves.toBe(42);
        expect(await xfs.existsPromise(markerPath)).toBe(true);
        const stored = await readTreeHashes(run, true);
        await run(`install`, {enableWorkspaceHashes: false});
        await xfs.removePromise(markerPath);
        expect(await readTreeHashes(run, false)).toEqual(stored);
        expect(await xfs.existsPromise(markerPath)).toBe(false);

        // Leave the generator and manifests intact, but remove every artifact cache.
        // On a cold cache, on-demand hash computation prepares the missing artifact.
        await xfs.removePromise(`${path}/.yarn/cache` as PortablePath);
        await xfs.removePromise(`${path}/.yarn/global` as PortablePath);
        expect(await readTreeHashes(run, false)).toEqual(stored);
        expect(await xfs.existsPromise(markerPath)).toBe(true);
      }),
    );

    for (const enableWorkspaceHashes of [true, false]) {
      for (const input of [`patch file`, `file: folder`, `absolute file: folder`]) {
        test(
          `--since uses historical ${input} contents with unchanged manifests (hashes ${enableWorkspaceHashes})`,
          makeTemporaryMonorepoEnv(
            {private: true, workspaces: [`packages/*`]},
            {
              [`packages/workspace-a`]: {
                name: `workspace-a`,
                scripts: {print: `echo Test Workspace A`},
                dependencies: {[`no-deps`]: input === `patch file` ? `patch:no-deps@1.0.0#~/no-deps.patch` : `file:../../vendor/no-deps`},
              },
              [`packages/workspace-b`]: {
                name: `workspace-b`,
                scripts: {print: `echo Test Workspace B`},
              },
            },
            async ({path, run, source}) => {
              const inputPath = input === `patch file` ? `no-deps.patch` : `vendor/no-deps/index.js`;
              const before = input === `patch file` ? NO_DEPS_PATCH : `module.exports = {hello: 'before'};\n`;
              if (input === `absolute file: folder`) {
                const workspacePath = `${path}/packages/workspace-a` as PortablePath;
                const manifest = await yarn.readManifest(workspacePath);
                manifest.dependencies[`no-deps`] = `file:${path}/vendor/no-deps`;
                await yarn.writeManifest(workspacePath, manifest);
              }
              if (input !== `patch file`)
                await xfs.copyPromise(`${path}/vendor/no-deps` as PortablePath, npath.toPortablePath(await tests.getPackageDirectoryPath(`no-deps`, `1.0.0`)));
              await xfs.writeFilePromise(`${path}/${inputPath}` as PortablePath, before);
              await run(`install`, {enableWorkspaceHashes});
              expect(`workspaces` in await readLockfile(path)).toBe(enableWorkspaceHashes);
              const git = await gitInit(path, `Historical package contents`);

              // Only package contents change, outside both workspace directories.
              // Reading the live file for the old graph would incorrectly select nothing.
              await xfs.writeFilePromise(`${path}/${inputPath}` as PortablePath, before.replace(`before`, `after`));
              await run(`install`, {enableWorkspaceHashes});
              await expect(source(`require('no-deps').hello`, {cwd: `${path}/packages/workspace-a`, enableWorkspaceHashes})).resolves.toBe(`after`);
              expect(`workspaces` in await readLockfile(path)).toBe(enableWorkspaceHashes);
              expect((await git(`diff`, `HEAD`, `--name-only`, `--`, `package.json`, `packages`, `vendor`, `no-deps.patch`)).stdout.trim()).toBe(inputPath);
              const expectedStdout = (!enableWorkspaceHashes && input.includes(`absolute file:`))
                ? [`Test Workspace A`, `Test Workspace B`]
                : [`Test Workspace A`];
              await expectForEachSince(run, expectedStdout, {enableWorkspaceHashes});

              // Historical absolute sources cannot be replayed from checkout without hashes,
              // so reinstall cannot erase the comparison trigger.
              await xfs.writeFilePromise(`${path}/${inputPath}` as PortablePath, before);
              await run(`install`, {enableWorkspaceHashes});
              await xfs.writeFilePromise(`${path}/README.md` as PortablePath, `Trigger historical comparison\n`);
              const expectedReadmeStdout = (!enableWorkspaceHashes && input.includes(`absolute file:`))
                ? [`Test Workspace A`, `Test Workspace B`]
                : [];
              await expectForEachSince(run, expectedReadmeStdout, {enableWorkspaceHashes});
            },
          ),
        );
      }

      for (const workspaceOnly of [false, true]) {
        test(
          `--since selects an untouched dependent of a moved workspace (hashes ${enableWorkspaceHashes}, workspace-only ${workspaceOnly})`,
          makeTemporaryMonorepoEnv(
            {private: true, workspaces: [`packages/*`]},
            {
              [`packages/workspace-a`]: {
                name: `workspace-a`,
                scripts: {print: `echo Test Workspace A`},
                dependencies: {[`workspace-c`]: `workspace:*`},
              },
              [`packages/workspace-b`]: {
                name: `workspace-b`,
                scripts: {print: `echo Test Workspace B`},
              },
              [`packages/workspace-c`]: {
                name: `workspace-c`,
                scripts: {print: `echo Test Workspace C`},
                dependencies: workspaceOnly ? {} : {[`no-deps`]: `1.0.0`},
              },
            },
            async ({path, run}) => {
              await run(`install`, {enableWorkspaceHashes});
              const lockfileBefore = await readLockfile(path);
              const git = await gitInit(path, `Before move`);

              await git(`mv`, `packages/workspace-c`, `packages/workspace-c-renamed`);
              const manifestPath = `${path}/packages/workspace-c-renamed/package.json` as PortablePath;
              const manifest = await xfs.readJsonPromise(manifestPath);
              manifest.dependencies = workspaceOnly
                ? {[`workspace-b`]: `workspace:*`}
                : {[`no-deps`]: `2.0.0`};
              await fs.writeJson(manifestPath, manifest);
              await run(`install`, {enableWorkspaceHashes});

              if (workspaceOnly && !enableWorkspaceHashes)
                expect(await readLockfile(path)).toEqual(lockfileBefore);

              expect((await git(`diff`, `HEAD`, `--`, `packages/workspace-a/package.json`, `packages/workspace-b/package.json`)).stdout).toBe(``);
              // No --recursive: A must be selected by its changed dependency tree,
              // not merely because C's moved manifest was attributed to C.
              await expectForEachSince(run, [`Test Workspace A`, `Test Workspace C`], {enableWorkspaceHashes});
            },
          ),
        );
      }

      test(
        `--since uses historical workspace patterns when configuration is unchanged (hashes ${enableWorkspaceHashes})`,
        makeTemporaryMonorepoEnv(
          {
            private: true,
            workspaces: [`packages/*`, `legacy/*`, `!legacy/zz-not-a-workspace`],
          },
          {
            [`packages/workspace-a`]: {
              name: `workspace-a`,
              scripts: {print: `echo Test Workspace A`},
              dependencies: {[`workspace-c`]: `workspace:*`},
            },
            [`packages/workspace-b`]: {
              name: `workspace-b`,
              scripts: {print: `echo Test Workspace B`},
            },
            [`legacy/workspace-c`]: {
              name: `workspace-c`,
              scripts: {print: `echo Test Workspace C`},
              dependencies: {[`no-deps`]: `catalog:runtime`},
            },
            // A decoy with the same name must never replace the real historical C.
            [`legacy/zz-not-a-workspace`]: {
              name: `workspace-c`,
              dependencies: {[`no-deps`]: `2.0.0`},
            },
          },
          async ({path, run}) => {
            await yarn.writeConfiguration(path, {catalogs: {runtime: {[`no-deps`]: `1.0.0`}}});
            await run(`install`, {enableWorkspaceHashes});
            const git = await gitInit(path, `Historical workspace patterns`);

            // Move C to packages/* and change its dependency without changing .yarnrc.yml.
            await git(`mv`, `legacy/workspace-c`, `packages/workspace-c`);
            const rootManifestPath = `${path}/package.json` as PortablePath;
            const rootManifest = await xfs.readJsonPromise(rootManifestPath);
            rootManifest.workspaces = [`packages/*`];
            await fs.writeJson(rootManifestPath, rootManifest);

            const cManifestPath = `${path}/packages/workspace-c/package.json` as PortablePath;
            const cManifest = await xfs.readJsonPromise(cManifestPath);
            cManifest.dependencies[`no-deps`] = `2.0.0`;
            await fs.writeJson(cManifestPath, cManifest);

            await run(`install`, {enableWorkspaceHashes});

            expect((await git(`diff`, `HEAD`, `--`, `packages/workspace-a/package.json`, `packages/workspace-b/package.json`)).stdout).toBe(``);
            await expectForEachSince(run, [`Test Workspace A`, `Test Workspace C`], {enableWorkspaceHashes});
          },
        ),
      );

      test(
        `--since marks newly included workspace as changed when workspace patterns expand (hashes ${enableWorkspaceHashes})`,
        makeTemporaryMonorepoEnv(
          {
            private: true,
            workspaces: [`packages/workspace-a`],
          },
          {
            [`packages/workspace-a`]: {
              name: `workspace-a`,
              scripts: {print: `echo Test Workspace A`},
            },
            [`packages/workspace-b`]: {
              name: `workspace-b`,
              scripts: {print: `echo Test Workspace B`},
            },
          },
          async ({path, run}) => {
            await run(`install`, {enableWorkspaceHashes});
            await gitInit(path, `Initial commit`);

            // workspace-b directory was already tracked in git, but wasn't a workspace.
            // Expand workspace patterns to include workspace-b.
            const manifestPath = `${path}/package.json` as PortablePath;
            const manifest = await xfs.readJsonPromise(manifestPath);
            manifest.workspaces = [`packages/*`];
            await fs.writeJson(manifestPath, manifest);
            await run(`install`, {enableWorkspaceHashes});

            // workspace-b must be marked changed because it is newly included as a workspace,
            // while workspace-a is unchanged.
            await expectForEachSince(run, [`Test Workspace B`], {enableWorkspaceHashes});
          },
        ),
      );
    }

    test(
      `git and url dependencies don't disable a workspace's on-demand tree hash`,
      makeTemporaryMonorepoEnv(
        {
          private: true,
          workspaces: [`packages/*`],
        },
        {
          [`packages/workspace-a`]: {
            name: `workspace-a`,
            version: `1.0.0`,
            scripts: {print: `echo Test Workspace A`},
            dependencies: {
              [`no-prepack`]: `__GIT_URL__`,
              [`one-range-dep`]: `1.0.0`,
            },
          },
          [`packages/workspace-b`]: {
            name: `workspace-b`,
            version: `1.0.0`,
            scripts: {print: `echo Test Workspace B`},
            dependencies: {
              [`has-bin-entries`]: `__SERVER_URL__/package.tgz`,
            },
          },
        },
        async ({path, run}) => {
          const gitUrl = await tests.startPackageServer().then(url => `${url}/repositories/no-prepack.git`);
          const registryHost = new URL(await tests.startPackageServer()).hostname;

          const archive = await xfs.readFilePromise(await tests.getPackageArchivePath(`has-bin-entries`, `1.0.0`));
          const server = await startServer((_request, response) => {
            response.writeHead(200, {
              [`Connection`]: `close`,
              [`Content-Length`]: archive.length,
            });
            response.end(archive);
          });

          const urlConfig = {unsafeHttpWhitelist: [registryHost, `127.0.0.1`]};

          // Substitute URLs into manifests
          const manifestA = await yarn.readManifest(`${path}/packages/workspace-a` as PortablePath);
          manifestA.dependencies[`no-prepack`] = gitUrl;
          await yarn.writeManifest(`${path}/packages/workspace-a` as PortablePath, manifestA);

          const manifestB = await yarn.readManifest(`${path}/packages/workspace-b` as PortablePath);
          manifestB.dependencies[`has-bin-entries`] = `${server.url}/package.tgz`;
          await yarn.writeManifest(`${path}/packages/workspace-b` as PortablePath, manifestB);

          try {
            await tests.setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
              await run(`install`, {enableWorkspaceHashes: false, ...urlConfig});
            });

            // The git and url dependencies are recorded in the lockfile,
            // so the on-demand walk must resolve them like any other edge
            // and still compute both workspaces' tree hashes.
            const hashes = await readTreeHashes(run, false, urlConfig);
            expect(hashes[`workspace-a`]).toMatch(/^[0-9a-f]+$/);
            expect(hashes[`workspace-b`]).toMatch(/^[0-9a-f]+$/);

            await gitInit(path, `First commit`);

            // Make no-deps@1.1.0 visible and upgrade; only the lockfile
            // changes, and only workspace-a's dependency tree changed
            // through it (via one-range-dep, next to its git dep).
            await tests.setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`, `1.1.0`])]]), async () => {
              await run(`up`, `-R`, `no-deps`, {enableWorkspaceHashes: false, ...urlConfig});
            });

            // The old-side walk must attribute the lockfile change to
            // workspace-a even though its tree also contains the git
            // dependency; workspace-b's tree didn't change, so it must not run.
            await expectForEachSince(run, [`Test Workspace A`], {enableWorkspaceHashes: false, ...urlConfig});
          } finally {
            await server.close();
          }
        },
      ),
    );
  });
});
