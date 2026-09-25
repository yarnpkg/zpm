import {Filename, ppath, xfs} from '@yarnpkg/fslib';
import {tests}                from 'pkg-tests-core';

const {setPackageWhitelist} = tests;

describe(`Features`, () => {
  describe(`enableAutoDedupe`, () => {
    for (const command of [[`install`], [`install`, `--silent`], [`install`, `--mode=update-lockfile`], [`add`, `no-deps@1.1.0`], [`up`, `no-deps@1.1.0`], [`remove`, `one-range-dep-too`]]) {
      it(
        `should dedupe after ${command.join(` `)}`,
        makeTemporaryEnv({}, async ({path, run, source}) => {
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
