const {
  fs: {writeFile, writeJson},
  exec: {execFile},
  tests: {setPackageWhitelist},
} = require(`pkg-tests-core`);

describe(`Commands`, () => {
  describe(`version check`, () => {
    test(
      `it should pass for basic repositories`,
      makeVersionCheckEnv(async ({path, run, source, git}) => {
        await run(`version`, `check`);
      }),
    );

    test(
      `it should detect that a workspace changed vs master`,
      makeVersionCheckEnv(async ({path, run, source, git}) => {
        await git(`checkout`, `-b`, `my-feature`);

        await writeJson(`${path}/packages/pkg-c/wip.json`, {});

        await expect(run(`version`, `check`)).rejects.toThrow();
      }),
    );

    test(
      `it shouldn't detect a change when pulling master`,
      makeVersionCheckEnv(async ({path, run, source, git}) => {
        // First we create a copy of master
        await git(`checkout`, `-b`, `my-feature`);

        // Then we push some changes on master
        await git(`checkout`, `master`);
        await writeJson(`${path}/packages/pkg-c/wip.json`, {});
        await git(`add`, `.`);
        await git(`commit`, `-m`, `wip`);

        // Then we merge master into our feature branch
        await git(`checkout`, `my-feature`);
        await git(`merge`, `master`);

        // Our feature branch shouldn't report `pkg-c` as potentially requiring
        // a bump, since we haven't modified it ourselves
        await run(`version`, `check`);
      }),
    );

    test(
      `it shouldn't throw if a modified workspace has been bumped`,
      makeVersionCheckEnv(async ({path, run, source, git}) => {
        await git(`checkout`, `-b`, `my-feature`);

        await writeJson(`${path}/packages/pkg-c/wip.json`, {});
        await run(`packages/pkg-c`, `version`, `patch`, `--deferred`);

        await run(`version`, `check`);
      }),
    );

    test(
      `it shouldn't throw if a modified workspace has declined to be bumped`,
      makeVersionCheckEnv(async ({path, run, source, git}) => {
        await git(`checkout`, `-b`, `my-feature`);

        await writeJson(`${path}/packages/pkg-c/wip.json`, {});
        await run(`packages/pkg-c`, `version`, `decline`, `--deferred`);

        await run(`version`, `check`);
      }),
    );

    test(
      `it should detect that a dependent workspace changed vs master`,
      makeVersionCheckEnv(async ({path, run, source, git}) => {
        await git(`checkout`, `-b`, `my-feature`);

        await writeJson(`${path}/packages/pkg-a/wip.json`, {});
        await run(`packages/pkg-a`, `version`, `patch`, `--deferred`);

        await expect(run(`version`, `check`)).rejects.toThrow();
      }),
    );

    test(
      `it should detect that a dependent workspace changed vs master, even when the current workspace already has a bump scheduled`,
      makeVersionCheckEnv(async ({path, run, source, git}) => {
        await run(`packages/pkg-c`, `version`, `patch`, `--deferred`);

        await git(`add`, `.`);
        await git(`commit`, `-m`, `Bumping pkg-c`);

        await git(`checkout`, `-b`, `my-feature`);

        await writeJson(`${path}/packages/pkg-a/wip.json`, {});
        await run(`packages/pkg-a`, `version`, `patch`, `--deferred`);

        await expect(run(`version`, `check`)).rejects.toThrow();
      }),
    );

    test(
      `it shouldn't throw if a dependent workspace has been bumped`,
      makeVersionCheckEnv(async ({path, run, source, git}) => {
        await git(`checkout`, `-b`, `my-feature`);

        await writeJson(`${path}/packages/pkg-a/wip.json`, {});
        await run(`packages/pkg-a`, `version`, `decline`, `--deferred`);
        await run(`packages/pkg-c`, `version`, `patch`, `--deferred`);

        await run(`version`, `check`);
      }),
    );

    test(
      `it shouldn't throw if a dependent workspace has declined to be bumped`,
      makeVersionCheckEnv(async ({path, run, source, git}) => {
        await git(`checkout`, `-b`, `my-feature`);

        await writeJson(`${path}/packages/pkg-a/wip.json`, {});
        await run(`packages/pkg-a`, `version`, `decline`, `--deferred`);
        await run(`packages/pkg-c`, `version`, `decline`, `--deferred`);

        await run(`version`, `check`);
      }),
    );

    test(
      `it should require a decision from the root workspace when the lockfile changes its dependency tree`,
      makeVersionCheckEnv(async ({path, run, git}) => {
        await git(`checkout`, `-b`, `my-feature`);

        // The bump decision for pkg-c is stored inside the root workspace
        // (that's where the deferred version folder lives), which is what
        // used to mask the lockfile change below
        await writeJson(`${path}/packages/pkg-c/wip.json`, {});
        await run(`packages/pkg-c`, `version`, `patch`, `--deferred`);

        await setPackageWhitelist(new Map([[`no-deps`, new Set([`1.0.0`, `1.1.0`])]]), async () => {
          await run(`up`, `-R`, `no-deps`);
        });

        // The root's own dependency tree changed, so it needs a decision too
        await expect(run(`version`, `check`)).rejects.toThrow();

        await run(`version`, `decline`, `--deferred`);
        await run(`version`, `check`);
      }, {
        // A *transitive* dependency, so that `yarn up` has no range to
        // rewrite in the root manifest: the lockfile is the only thing
        // that changes
        rootManifest: {dependencies: {[`one-range-dep`]: `1.0.0`}},
        packageWhitelist: new Map([[`no-deps`, new Set([`1.0.0`])]]),
        // Install artifacts live in the root workspace; were they visible to
        // git, they'd flag the root on their own and the test would prove
        // nothing. The deferred version files must stay visible, though.
        gitignore: [`.yarn/*`, `!.yarn/versions`, `.pnp.*`, `node_modules`],
      }),
    );

    test(
      `it shouldn't throw if changes were reverted`,
      makeVersionCheckEnv(async ({path, run, source, git}) => {
        await writeFile(`${path}/packages/pkg-a/state`, `Initial`);
        await git(`add`, `.`);
        await git(`commit`, `-m`, `Initial state`);

        await git(`checkout`, `-b`, `feature-branch`);

        await writeFile(`${path}/packages/pkg-a/state`, `Next`);
        await git(`commit`, `-am`, `WIP`);

        await writeFile(`${path}/packages/pkg-a/state`, `Initial`);
        await git(`commit`, `-am`, `Revert WIP`);

        await expect(run(`version`, `check`)).resolves.toBeTruthy();
      }),
    );
  });
});

function makeVersionCheckEnv(cb, {rootManifest = {}, packageWhitelist = null, gitignore = null} = {}) {
  return makeTemporaryEnv({
    private: true,
    workspaces: [`packages/*`],
    ...rootManifest,
  }, async ({path, run, ...rest}) => {
    const git = (...args) => execFile(`git`, args, {cwd: path});

    await writeJson(`${path}/packages/pkg-a/package.json`, {
      name: `pkg-a`,
      version: `1.0.0`,
    });

    await writeJson(`${path}/packages/pkg-b/package.json`, {
      name: `pkg-b`,
      version: `1.0.0`,
    });

    await writeJson(`${path}/packages/pkg-c/package.json`, {
      name: `pkg-c`,
      version: `1.0.0`,
      dependencies: {
        [`pkg-a`]: `workspace:1.0.0`,
        [`pkg-b`]: `workspace:1.0.0`,
      },
    });

    if (packageWhitelist !== null) {
      await setPackageWhitelist(packageWhitelist, async () => {
        await run(`install`);
      });
    } else {
      await run(`install`);
    }

    if (gitignore !== null)
      await writeFile(`${path}/.gitignore`, `${gitignore.join(`\n`)}\n`);

    await git(`init`, `--initial-branch=master`);

    // Otherwise we can't always commit
    await git(`config`, `user.name`, `John Doe`);
    await git(`config`, `user.email`, `john.doe@example.org`);
    await git(`config`, `commit.gpgSign`, `false`);

    await git(`add`, `.`);
    await git(`commit`, `-m`, `First commit`);

    await cb({path, run, ...rest, git});
  });
}
