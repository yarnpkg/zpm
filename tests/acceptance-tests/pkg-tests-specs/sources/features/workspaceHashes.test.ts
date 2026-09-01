import {PortablePath, xfs} from '@yarnpkg/fslib';

async function readLockfile(path: PortablePath) {
  const raw = await xfs.readFilePromise(`${path}/yarn.lock` as PortablePath, `utf8`);
  return JSON.parse(raw);
}

function getWorkspaceHashes(lockfile: any): Record<string, string> {
  return lockfile.workspaces ?? {};
}

describe(`Features`, () => {
  describe(`Workspace hashes`, () => {
    test(
      `the lockfile stores one dependency tree hash per workspace by default`,
      makeTemporaryMonorepoEnv(
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
        async ({path, run}) => {
          await run(`install`);

          const lockfile = await readLockfile(path);

          expect(Object.keys(getWorkspaceHashes(lockfile)).sort()).toEqual([
            `root-workspace`,
            `workspace-a`,
            `workspace-b`,
          ]);

          for (const hash of Object.values(getWorkspaceHashes(lockfile)))
            expect(hash).toMatch(/^[0-9a-f]+$/);
        },
      ),
    );

    test(
      `the workspaces section is omitted when enableWorkspaceHashes is false, and the lockfile without it still parses`,
      makeTemporaryMonorepoEnv(
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
        async ({path, run}) => {
          await run(`install`, {enableWorkspaceHashes: false});

          const lockfile = await readLockfile(path);
          expect(`workspaces` in lockfile).toBe(false);

          // The lockfile without the section still parses and the
          // install stays fresh rather than looping.
          const second = await run(`install`, {enableWorkspaceHashes: false});
          expect(second.stdout).toContain(`up-to-date`);
        },
      ),
    );

    test(
      `--tree-hash returns the same hashes whether they are stored or computed on demand`,
      makeTemporaryMonorepoEnv(
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
        async ({path, run}) => {
          // Setting off: no stored section, hashes computed on demand.
          await run(`install`, {enableWorkspaceHashes: false});
          const onDemand = await run(`workspaces`, `list`, `--json`, `--tree-hash`);

          // Setting on: the section comes back (the old format with the
          // key is restored) and stores the very same hashes.
          await run(`install`);
          const stored = await run(`workspaces`, `list`, `--json`, `--tree-hash`);

          expect(stored.stdout).toEqual(onDemand.stdout);

          const lockfile = await readLockfile(path);
          expect(Object.keys(getWorkspaceHashes(lockfile)).sort()).toEqual([
            `root-workspace`,
            `workspace-a`,
            `workspace-b`,
          ]);

          const printed = new Map();
          for (const line of stored.stdout.split(`\n`)) {
            if (line === ``)
              continue;

            const payload = JSON.parse(line);
            if (payload.name !== null)
              printed.set(payload.name, payload.treeHash);
          }

          for (const [name, hash] of Object.entries(getWorkspaceHashes(lockfile))) {
            // The root workspace prints no name in the JSON stream; its
            // hash is still covered by the stdout equality above.
            if (name === `root-workspace`)
              continue;

            expect(printed.get(name)).toEqual(hash);
          }
        },
      ),
    );
  });
});
