import {Filename, PortablePath, ppath, xfs} from '@yarnpkg/fslib';
import {tests}                from 'pkg-tests-core';

const {setPackageWhitelist} = tests;

describe(`Features`, () => {
  describe(`enableAutoDedupe`, () => {
    for (const command of [[`install`], [`install`, `--silent`], [`install`, `--mode=update-lockfile`], [`run`, `check`], [`node`, `-e`, ``], [`add`, `no-deps@1.1.0`], [`up`, `no-deps@1.1.0`], [`remove`, `one-range-dep-too`]]) {
      it(
        `should dedupe after ${command.join(` `)}`,
        makeTemporaryEnv({scripts: {check: `node -e ""`}}, async ({path, run, source}) => {
          await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
            await run(`add`, `one-range-dep`, `one-range-dep-too`);
          });
          await run(`add`, `no-deps@1.1.0`);

          await expect(run(`dedupe`, `--check`)).rejects.toMatchObject({code: 1});
          await run(...command, {enableAutoDedupe: true});

          if (command.includes(`--mode=update-lockfile`)) {
            const lockfile = await xfs.readJsonPromise(ppath.join(path, Filename.lockfile));
            expect(lockfile.entries[`no-deps@npm:1.1.0, no-deps@npm:^1.0.0`].resolution.version).toEqual(`1.1.0`);
            // Lockfile-only installs intentionally leave the linked tree unchanged.
            await expect(source(`require('one-range-dep').dependencies['no-deps'].version`)).resolves.toEqual(`1.0.0`);
            await run(`install`, `--force`);
          }

          await expect(run(`dedupe`, `--check`)).resolves.toMatchObject({code: 0});
          await expect(source(`require('one-range-dep').dependencies['no-deps'].version`)).resolves.toEqual(`1.1.0`);
        }),
      );
    }

    for (const command of [[`install`], [`run`, `check`]]) {
      it(
        `should skip a repeated install after ${command.join(` `)} deduplicates the project`,
        makeTemporaryEnv({scripts: {check: `node -e ""`}}, async ({path, run}) => {
          await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
            await run(`add`, `one-range-dep`);
          });
          await run(`add`, `no-deps@1.1.0`);
          await run(...command, {enableAutoDedupe: true});

          const lockfilePath = ppath.join(path, Filename.lockfile);
          const before = await xfs.readFilePromise(lockfilePath, `utf8`);
          const {stdout} = await run(`install`, {enableAutoDedupe: true});
          expect(stdout).toContain(`All dependencies are up-to-date, nothing to do.`);
          expect(await xfs.readFilePromise(lockfilePath, `utf8`)).toEqual(before);
          await expect(run(`dedupe`, `--check`, {enableAutoDedupe: true})).resolves.toMatchObject({code: 0});
        }),
      );
    }

    it(
      `should preserve explicit dedupe checks and patterns before a full install`,
      makeTemporaryEnv({}, {enableAutoDedupe: true}, async ({run, source}) => {
        await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
          await run(`add`, `one-range-dep`, {enableAutoDedupe: false});
        });
        await run(`add`, `no-deps@1.1.0`, {enableAutoDedupe: false});

        await expect(run(`dedupe`, `--check`)).rejects.toMatchObject({code: 1});
        await run(`dedupe`, `unrelated-package`);
        await expect(run(`dedupe`, `--check`)).rejects.toMatchObject({code: 1});

        await run(`install`);
        await expect(source(`require('one-range-dep').dependencies['no-deps'].version`)).resolves.toEqual(`1.1.0`);
      }),
    );

    it(
      `should preserve focused resolutions and dedupe when installing the full project`,
      makeTemporaryEnv({private: true, workspaces: [`workspace`]}, async ({path, run, source}) => {
        const workspacePath = ppath.join(path, `workspace` as PortablePath);
        await xfs.mkdirpPromise(workspacePath);
        await xfs.writeJsonPromise(ppath.join(workspacePath, Filename.manifest), {name: `workspace`});
        await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
          await run(`add`, `one-range-dep`, {cwd: workspacePath});
        });
        await run(`add`, `no-deps@1.1.0`, {cwd: workspacePath});

        const lockfilePath = ppath.join(path, Filename.lockfile);
        const before = await xfs.readFilePromise(lockfilePath, `utf8`);
        await run(`workspaces`, `focus`, `workspace`, {enableAutoDedupe: true});
        expect(await xfs.readFilePromise(lockfilePath, `utf8`)).toEqual(before);

        await run(`install`, {enableAutoDedupe: true});
        await expect(source(`require('one-range-dep').dependencies['no-deps'].version`, {cwd: workspacePath})).resolves.toEqual(`1.1.0`);
      }),
    );

    it(
      `should reuse versions introduced by add`,
      makeTemporaryEnv({}, {enableAutoDedupe: true}, async ({run, source}) => {
        await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
          await run(`add`, `one-range-dep`, `one-range-dep-too`);
        });
        await run(`add`, `no-deps@1.1.0`);
        await expect(source(`require('one-range-dep').dependencies['no-deps'].version`)).resolves.toEqual(`1.1.0`);
        await expect(run(`dedupe`, `--check`)).resolves.toMatchObject({code: 0});
      }),
    );

    it(
      `should allow disabling automatic deduplication`,
      makeTemporaryEnv({}, {enableAutoDedupe: true}, async ({run}) => {
        await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
          await run(`add`, `one-range-dep`);
        });
        await run(`add`, `no-deps@1.1.0`, {enableAutoDedupe: false});
        await expect(run(`dedupe`, `--check`)).rejects.toMatchObject({code: 1});
      }),
    );

    it(
      `should preserve incompatible dependency ranges`,
      makeTemporaryEnv({}, {enableAutoDedupe: true}, async ({run, source}) => {
        await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
          await run(`add`, `one-range-dep`);
        });
        await run(`add`, `no-deps@2.0.0`);
        await expect(source(`require('one-range-dep').dependencies['no-deps'].version`)).resolves.toEqual(`1.0.0`);
        await expect(source(`require('no-deps').version`)).resolves.toEqual(`2.0.0`);
      }),
    );

    it(
      `should respect immutable installs`,
      makeTemporaryEnv({}, async ({path, run}) => {
        await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`])]]), async () => {
          await run(`add`, `one-range-dep`, `one-range-dep-too`);
        });
        await run(`add`, `no-deps@1.1.0`);
        const lockfilePath = ppath.join(path, Filename.lockfile);
        const before = await xfs.readFilePromise(lockfilePath, `utf8`);

        await expect(run(`install`, `--immutable`, {enableAutoDedupe: true})).rejects.toMatchObject({code: 1});
        expect(await xfs.readFilePromise(lockfilePath, `utf8`)).toEqual(before);
      }),
    );
  });
});
