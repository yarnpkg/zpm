import {Filename, ppath, xfs} from '@yarnpkg/fslib';
import {exec, tests}          from 'pkg-tests-core';

function cleanLockfile(lockfile: string) {
  lockfile = lockfile.replace(/(^ {2}version: )[0-9]+$/m, `$1X`);
  lockfile = lockfile.replace(/(checksum: ).*/g, `$1<checksum stripped>`);
  lockfile = lockfile.replace(/(>>>>>>>).*(\(commit-[0-9].0.0\))/g, `$1 0000000 $2`);

  return lockfile;
}

function cleanRunResult<T extends {stdout: string}>(result: T): T {
  return {
    ...result,
    stdout: result.stdout.replace(/^➤ · Yarn .*\n/m, `➤ · Yarn <version>\n`),
  };
}

describe(`Features`, () => {
  describe(`Merge Conflict Resolution`, () => {
    test(
      `it should properly fix merge conflicts`,
      makeTemporaryEnv(
        {},
        async ({path, run, source}) => {
          await exec.execGitInit({cwd: path});

          await run(`install`);

          await xfs.writeJsonPromise(ppath.join(path, Filename.manifest), {
            dependencies: {
              [`no-deps`]: `*`,
            },
          });

          await exec.execFile(`git`, [`add`, `-A`], {cwd: path});
          await exec.execFile(`git`, [`commit`, `-a`, `-m`, `my-commit`], {cwd: path});

          await exec.execFile(`git`, [`checkout`, `master`], {cwd: path});
          await exec.execFile(`git`, [`checkout`, `-b`, `1.0.0`], {cwd: path});

          await run(`set`, `resolution`, `no-deps@npm:*`, `npm:1.0.0`);
          const expectedV1Lockfile = await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`);

          await exec.execFile(`git`, [`add`, `-A`], {cwd: path});
          await exec.execFile(`git`, [`commit`, `-a`, `-m`, `commit-1.0.0`], {cwd: path});

          await exec.execFile(`git`, [`checkout`, `master`], {cwd: path});
          await exec.execFile(`git`, [`checkout`, `-b`, `2.0.0`], {cwd: path});

          await run(`set`, `resolution`, `no-deps@npm:*`, `npm:2.0.0`);
          const expectedV2Lockfile = await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`);

          await exec.execFile(`git`, [`add`, `-A`], {cwd: path});
          await exec.execFile(`git`, [`commit`, `-a`, `-m`, `commit-2.0.0`], {cwd: path});

          /* Merging 1.0 first, then 2.0 */{
            await exec.execFile(`git`, [`checkout`, `-b`, `merge-1-then-2`, `master`], {cwd: path});
            await exec.execFile(`git`, [`merge`, `1.0.0`], {cwd: path});

            await expect(exec.execFile(`git`, [`merge`, `2.0.0`], {cwd: path, env: {LC_ALL: `C`}})).rejects.toThrow(/CONFLICT/);

            const preFixLockfile = await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`);
            expect(preFixLockfile).toContain(`<<<<<<<`);

            await run(`install`);

            const expectedLockfile = tests.FEATURE_CHECKS.mergeConflictTheirs
              ? expectedV2Lockfile
              : expectedV1Lockfile;

            const postFixLockfile = await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`);
            expect(postFixLockfile).toEqual(expectedLockfile);

            await exec.execFile(`git`, [`merge`, `--abort`], {cwd: path});
          }

          /* Merging 2.0 first, then 1.0 */ {
            await exec.execFile(`git`, [`checkout`, `-b`, `merge-2-then-1`, `master`], {cwd: path});
            await exec.execFile(`git`, [`merge`, `2.0.0`], {cwd: path});

            await expect(exec.execFile(`git`, [`merge`, `1.0.0`], {cwd: path, env: {LC_ALL: `C`}})).rejects.toThrow(/CONFLICT/);

            const preFixLockfile = await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`);
            expect(preFixLockfile).toContain(`<<<<<<<`);

            await run(`install`);

            const expectedLockfile = tests.FEATURE_CHECKS.mergeConflictTheirs
              ? expectedV1Lockfile
              : expectedV2Lockfile;

            const postFixLockfile = await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`);
            expect(postFixLockfile).toEqual(expectedLockfile);
          }
        },
      ),
    );

    test(
      `it should support fixing rebase conflicts`,
      makeTemporaryEnv(
        {},
        async ({path, run, source}) => {
          await exec.execGitInit({cwd: path});

          await run(`install`);
          await xfs.writeJsonPromise(ppath.join(path, Filename.manifest), {dependencies: {[`no-deps`]: `*`}});

          await exec.execFile(`git`, [`add`, `-A`], {cwd: path});
          await exec.execFile(`git`, [`commit`, `-a`, `-m`, `my-commit`], {cwd: path});

          await exec.execFile(`git`, [`checkout`, `master`], {cwd: path});
          await exec.execFile(`git`, [`checkout`, `-b`, `1.0.0`], {cwd: path});

          await run(`set`, `resolution`, `no-deps@npm:*`, `npm:1.0.0`);

          await exec.execFile(`git`, [`add`, `-A`], {cwd: path});
          await exec.execFile(`git`, [`commit`, `-a`, `-m`, `commit-1.0.0`], {cwd: path});

          await exec.execFile(`git`, [`checkout`, `master`], {cwd: path});
          await exec.execFile(`git`, [`checkout`, `-b`, `2.0.0`], {cwd: path});

          await run(`set`, `resolution`, `no-deps@npm:*`, `npm:2.0.0`);
          const expectedV2Lockfile = await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`);

          await exec.execFile(`git`, [`add`, `-A`], {cwd: path});
          await exec.execFile(`git`, [`commit`, `-a`, `-m`, `commit-2.0.0`], {cwd: path});

          await exec.execFile(`git`, [`checkout`, `master`], {cwd: path});
          await exec.execFile(`git`, [`merge`, `1.0.0`], {cwd: path});

          await expect(exec.execFile(`git`, [`rebase`, `2.0.0`], {cwd: path, env: {LC_ALL: `C`}})).rejects.toThrow(/CONFLICT/);

          const preFixLockfile = await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`);
          expect(preFixLockfile).toContain(`<<<<<<<`);

          await run(`install`);

          const postFixLockfile = await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`);
          expect(postFixLockfile).toEqual(expectedV2Lockfile);
        },
      ),
    );

    test(
      `it should support fixing cherry-pick conflicts`,
      makeTemporaryEnv(
        {},
        async ({path, run, source}) => {
          await exec.execGitInit({cwd: path});

          await run(`install`);
          await xfs.writeJsonPromise(ppath.join(path, Filename.manifest), {dependencies: {[`no-deps`]: `*`}});

          await exec.execFile(`git`, [`add`, `-A`], {cwd: path});
          await exec.execFile(`git`, [`commit`, `-a`, `-m`, `my-commit`], {cwd: path});

          await exec.execFile(`git`, [`checkout`, `master`], {cwd: path});
          await exec.execFile(`git`, [`checkout`, `-b`, `1.0.0`], {cwd: path});

          await run(`set`, `resolution`, `no-deps@npm:*`, `npm:1.0.0`);
          const expectedV1Lockfile = await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`);

          await exec.execFile(`git`, [`add`, `-A`], {cwd: path});
          await exec.execFile(`git`, [`commit`, `-a`, `-m`, `commit-1.0.0`], {cwd: path});

          await exec.execFile(`git`, [`checkout`, `master`], {cwd: path});
          await exec.execFile(`git`, [`checkout`, `-b`, `2.0.0`], {cwd: path});

          await run(`set`, `resolution`, `no-deps@npm:*`, `npm:2.0.0`);
          const expectedV2Lockfile = await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`);

          await exec.execFile(`git`, [`add`, `-A`], {cwd: path});
          await exec.execFile(`git`, [`commit`, `-a`, `-m`, `commit-2.0.0`], {cwd: path});

          await exec.execFile(`git`, [`checkout`, `master`], {cwd: path});
          await exec.execFile(`git`, [`merge`, `1.0.0`], {cwd: path});

          await expect(exec.execFile(`git`, [`cherry-pick`, `2.0.0`], {cwd: path, env: {LC_ALL: `C`}})).rejects.toThrow(/CONFLICT/);

          const preFixLockfile = await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`);
          expect(preFixLockfile).toContain(`<<<<<<<`);

          await run(`install`);

          const expectedLockfile = tests.FEATURE_CHECKS.mergeConflictTheirs
            ? expectedV2Lockfile
            : expectedV1Lockfile;

          const postFixLockfile = await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`);
          expect(postFixLockfile).toEqual(expectedLockfile);
        },
      ),
    );

    test(
      `it shouldn't re-fetch the lockfile metadata when performing simple merge conflict resolutions`,
      makeTemporaryEnv(
        {},
        async ({path, run, source}) => {
          await exec.execGitInit({cwd: path});

          await run(`install`);
          await xfs.writeJsonPromise(ppath.join(path, Filename.manifest), {dependencies: {[`no-deps`]: `*`}});

          await exec.execFile(`git`, [`add`, `-A`], {cwd: path});
          await exec.execFile(`git`, [`commit`, `-a`, `-m`, `my-commit`], {cwd: path});

          await exec.execFile(`git`, [`checkout`, `master`], {cwd: path});
          await exec.execFile(`git`, [`checkout`, `-b`, `1.0.0`], {cwd: path});
          await run(`set`, `resolution`, `no-deps@npm:*`, `npm:1.0.0`);
          await exec.execFile(`git`, [`add`, `-A`], {cwd: path});
          await exec.execFile(`git`, [`commit`, `-a`, `-m`, `commit-1.0.0`], {cwd: path});

          await exec.execFile(`git`, [`checkout`, `master`], {cwd: path});
          await exec.execFile(`git`, [`checkout`, `-b`, `2.0.0`], {cwd: path});
          await run(`set`, `resolution`, `no-deps@npm:*`, `npm:2.0.0`);
          await exec.execFile(`git`, [`add`, `-A`], {cwd: path});
          await exec.execFile(`git`, [`commit`, `-a`, `-m`, `commit-2.0.0`], {cwd: path});

          await exec.execFile(`git`, [`checkout`, `master`], {cwd: path});
          await exec.execFile(`git`, [`merge`, `1.0.0`], {cwd: path});

          await expect(exec.execFile(`git`, [`merge`, `2.0.0`], {cwd: path, env: {LC_ALL: `C`}})).rejects.toThrow(/CONFLICT/);

          const lockfile = await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`);
          expect(cleanLockfile(lockfile)).toMatchSnapshot();

          await expect(run(`install`, {
            enableNetwork: false,
          }).then(cleanRunResult)).resolves.toMatchSnapshot();
        },
      ),
    );
  });
});
