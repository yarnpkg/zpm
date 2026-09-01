import {PortablePath, xfs} from '@yarnpkg/fslib';
import {exec, fs, tests}   from 'pkg-tests-core';

import {RunFunction}       from '../../../pkg-tests-core/sources/utils/tests';

async function readLockfile(path: PortablePath) {
  const raw = await xfs.readFilePromise(`${path}/yarn.lock` as PortablePath, `utf8`);
  return JSON.parse(raw);
}

const forEachVerboseDone = tests.FEATURE_CHECKS.forEachVerboseDone
  ? []
  : [`Done\n`];

// A monorepo whose workspace-a depends on a registry package and
// workspace-b depends on workspace-a, so each workspace has a
// different dependency tree to hash.
const makeHashesEnv = (fn: RunFunction) => makeTemporaryMonorepoEnv(
  {
    private: true,
    workspaces: [`packages/*`],
  },
  {
    [`packages/workspace-a`]: {
      name: `workspace-a`,
      version: `1.0.0`,
      dependencies: {
        [`no-deps`]: `1.0.0`,
      },
    },
    [`packages/workspace-b`]: {
      name: `workspace-b`,
      version: `1.0.0`,
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
      `the lockfile stores one dependency tree hash per workspace by default`,
      makeHashesEnv(async ({path, run}) => {
        await run(`install`);

        const lockfile = await readLockfile(path);

        expect(Object.keys(lockfile.workspaces ?? {}).sort()).toEqual([
          `root-workspace`,
          `workspace-a`,
          `workspace-b`,
        ]);

        for (const hash of Object.values(lockfile.workspaces ?? {})) {
          expect(hash).toMatch(/^[0-9a-f]+$/);
        }
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
      `--tree-hash returns the same hashes whether they are stored or computed on demand`,
      makeHashesEnv(async ({path, run}) => {
        // Setting off: no stored section, hashes computed on demand.
        await run(`install`, {enableWorkspaceHashes: false});
        const onDemand = await run(`workspaces`, `list`, `--json`, `--tree-hash`);

        // Setting on: the section comes back storing the very same hashes.
        await run(`install`);
        const stored = await run(`workspaces`, `list`, `--json`, `--tree-hash`);

        expect(stored.stdout).toEqual(onDemand.stdout);
        expect(Object.keys((await readLockfile(path)).workspaces ?? {}).sort()).toEqual([
          `root-workspace`,
          `workspace-a`,
          `workspace-b`,
        ]);
      }),
    );

    test(
      `--since still attributes lockfile changes to the affected workspaces when enableWorkspaceHashes is false`,
      makeTemporaryEnv(
        {
          private: true,
          workspaces: [`packages/*`],
        },
        async ({path, run}) => {
          await fs.writeJson(`${path}/packages/workspace-a/package.json` as PortablePath, {
            name: `workspace-a`,
            version: `1.0.0`,
            scripts: {
              print: `echo Test Workspace A`,
            },
            dependencies: {
              [`one-range-dep`]: `1.0.0`,
            },
          });

          await fs.writeJson(`${path}/packages/workspace-b/package.json` as PortablePath, {
            name: `workspace-b`,
            version: `1.0.0`,
            scripts: {
              print: `echo Test Workspace B`,
            },
          });

          const git = (...args: Array<string>) => exec.execFile(`git`, args, {cwd: path});

          // Install with only no-deps@1.0.0 visible, so one-range-dep
          // resolves to no-deps@1.0.0.
          await tests.setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
            await run(`install`, {enableWorkspaceHashes: false});
          });

          await exec.execGitInit({cwd: path});
          await git(`add`, `-A`);
          await git(`commit`, `-m`, `First commit`);

          // Now make no-deps@1.1.0 visible and upgrade; only the
          // lockfile changes, and only workspace-a's dependency tree
          // changed through it.
          await tests.setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`, `1.1.0`])]]), async () => {
            await run(`up`, `-R`, `no-deps`, {enableWorkspaceHashes: false});
          });

          await expect(run(`workspaces`, `foreach`, `--since`, `run`, `print`)).resolves.toEqual({
            code: 0,
            stderr: ``,
            stdout: [
              `Test Workspace A\n`,
              ...forEachVerboseDone,
            ].join(``),
          });
        },
      ),
    );

    test(
      `toggling the setting between git refs doesn't flag untouched workspaces`,
      makeTemporaryEnv(
        {
          private: true,
          workspaces: [`packages/*`],
        },
        async ({path, run}) => {
          await fs.writeJson(`${path}/packages/workspace-a/package.json` as PortablePath, {
            name: `workspace-a`,
            version: `1.0.0`,
            scripts: {
              print: `echo Test Workspace A`,
            },
          });

          await fs.writeJson(`${path}/packages/workspace-b/package.json` as PortablePath, {
            name: `workspace-b`,
            version: `1.0.0`,
            scripts: {
              print: `echo Test Workspace B`,
            },
          });

          const git = (...args: Array<string>) => exec.execFile(`git`, args, {cwd: path});

          // Install with the setting on: the lockfile stores hashes.
          await run(`install`);

          await exec.execGitInit({cwd: path});
          await git(`add`, `-A`);
          await git(`commit`, `-m`, `Hashes on`);

          // Toggle off: the section disappears from the lockfile, but
          // no workspace actually changed, so nothing must run.
          await run(`install`, {enableWorkspaceHashes: false});

          // `foreach` reinstalls with the setting off too, otherwise
          // the default would restore the section before the check.
          await expect(run(`workspaces`, `foreach`, `--since`, `run`, `print`, {enableWorkspaceHashes: false})).resolves.toEqual({
            code: 0,
            stderr: ``,
            stdout: forEachVerboseDone.join(``),
          });

          // Toggle on again after committing the hashes-off lockfile:
          // the section comes back, still without flagging anything.
          await git(`add`, `-A`);
          await git(`commit`, `-m`, `Hashes off`);

          await run(`install`);

          await expect(run(`workspaces`, `foreach`, `--since`, `run`, `print`)).resolves.toEqual({
            code: 0,
            stderr: ``,
            stdout: forEachVerboseDone.join(``),
          });
        },
      ),
    );
  });
});
