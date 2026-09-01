import {PortablePath, xfs} from '@yarnpkg/fslib';

async function readLockfile(path: PortablePath) {
  const raw = await xfs.readFilePromise(`${path}/yarn.lock` as PortablePath, `utf8`);
  return JSON.parse(raw);
}

// A monorepo whose workspace-a depends on a registry package and
// workspace-b depends on workspace-a, so each workspace has a
// different dependency tree to hash.
const makeHashesEnv = (fn: any) => makeTemporaryMonorepoEnv(
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

        for (const hash of Object.values(lockfile.workspaces ?? {}))
          expect(hash).toMatch(/^[0-9a-f]+$/);
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
  });
});
