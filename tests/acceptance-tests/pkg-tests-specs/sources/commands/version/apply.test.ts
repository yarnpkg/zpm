import {xfs, ppath, Filename} from '@yarnpkg/fslib';

describe(`Commands`, () => {
  describe(`version apply`, () => {
    test(
      `it should apply the new version to the relevant package`,
      makeTemporaryEnv(
        {
          version: `0.0.0`,
        },
        async ({path, run}) => {
          await run(`version`, `patch`, `--deferred`);

          await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
            version: `0.0.0`,
          });

          await run(`version`, `apply`, `--dry-run`);

          await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
            version: `0.0.0`,
          });

          await run(`version`, `apply`);

          await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
            version: `0.0.1`,
          });
        },
      ),
    );

    test(
      `it should apply deferred versions from deferredVersionFolder when configured`,
      makeTemporaryEnv(
        {
          version: `0.0.0`,
        },
        async ({path, run}) => {
          const customFolder = ppath.join(path, `.custom-versions`);
          const defaultFolder = ppath.join(path, `.yarn/versions`);

          await xfs.writeFilePromise(ppath.join(path, `.yarnrc.yml`), `deferredVersionFolder: ./.custom-versions\n`);

          await run(`version`, `patch`, `--deferred`);

          await expect(xfs.existsPromise(defaultFolder)).resolves.toEqual(false);
          await expect(xfs.readdirPromise(customFolder)).resolves.toHaveLength(1);

          await run(`version`, `apply`);

          await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
            version: `0.0.1`,
          });
        },
      ),
    );

    test(
      `it should only apply the new version to the relevant packages`,
      makeTemporaryEnv(
        {
          private: true,
          workspaces: [
            `packages/*`,
          ],
        },
        async ({path, run}) => {
          const pkgA = ppath.join(path, `packages/pkg-a`);
          const pkgB = ppath.join(path, `packages/pkg-b`);

          await xfs.mkdirpPromise(pkgA);
          await xfs.mkdirpPromise(pkgB);

          await xfs.writeJsonPromise(ppath.join(pkgA, Filename.manifest), {
            name: `pkg-a`,
            version: `1.0.0`,
          });

          await xfs.writeJsonPromise(ppath.join(pkgB, Filename.manifest), {
            name: `pkg-b`,
            version: `1.0.0`,
          });

          await run(`version`, `patch`, `--deferred`, {
            cwd: pkgB,
          });

          await run(`version`, `apply`, `--all`);

          await expect(xfs.readJsonPromise(ppath.join(pkgA, Filename.manifest))).resolves.toMatchObject({
            version: `1.0.0`,
          });

          await expect(xfs.readJsonPromise(ppath.join(pkgB, Filename.manifest))).resolves.toMatchObject({
            version: `1.0.1`,
          });
        },
      ),
    );

    test(
      `it should resolve deferredVersionFolder from the project root when versioning from a workspace`,
      makeTemporaryEnv(
        {
          private: true,
          workspaces: [
            `packages/*`,
          ],
        },
        async ({path, run}) => {
          const pkgA = ppath.join(path, `packages/pkg-a`);
          const pkgB = ppath.join(path, `packages/pkg-b`);
          const rootDeferredFolder = ppath.join(path, `.custom-versions`);
          const workspaceDeferredFolder = ppath.join(pkgB, `.custom-versions`);

          await xfs.mkdirpPromise(pkgA);
          await xfs.mkdirpPromise(pkgB);

          await xfs.writeJsonPromise(ppath.join(pkgA, Filename.manifest), {
            name: `pkg-a`,
            version: `1.0.0`,
          });

          await xfs.writeJsonPromise(ppath.join(pkgB, Filename.manifest), {
            name: `pkg-b`,
            version: `1.0.0`,
          });

          await xfs.writeFilePromise(ppath.join(path, `.yarnrc.yml`), `deferredVersionFolder: ./.custom-versions\n`);

          await run(`version`, `patch`, `--deferred`, {
            cwd: pkgB,
          });

          await expect(xfs.readdirPromise(rootDeferredFolder)).resolves.toHaveLength(1);
          await expect(xfs.existsPromise(workspaceDeferredFolder)).resolves.toEqual(false);

          await run(`version`, `apply`, `--all`);

          await expect(xfs.readJsonPromise(ppath.join(pkgA, Filename.manifest))).resolves.toMatchObject({
            version: `1.0.0`,
          });

          await expect(xfs.readJsonPromise(ppath.join(pkgB, Filename.manifest))).resolves.toMatchObject({
            version: `1.0.1`,
          });
        },
      ),
    );

    test(
      `it should resolve deferredVersionFolder from the project root when provided via environment`,
      makeTemporaryEnv(
        {
          private: true,
          workspaces: [
            `packages/*`,
          ],
        },
        async ({path, run}) => {
          const pkgA = ppath.join(path, `packages/pkg-a`);
          const pkgB = ppath.join(path, `packages/pkg-b`);
          const rootDeferredFolder = ppath.join(path, `.custom-versions`);
          const workspaceDeferredFolder = ppath.join(pkgB, `.custom-versions`);
          const env = {
            YARN_DEFERRED_VERSION_FOLDER: `./.custom-versions`,
          };

          await xfs.mkdirpPromise(pkgA);
          await xfs.mkdirpPromise(pkgB);

          await xfs.writeJsonPromise(ppath.join(pkgA, Filename.manifest), {
            name: `pkg-a`,
            version: `1.0.0`,
          });

          await xfs.writeJsonPromise(ppath.join(pkgB, Filename.manifest), {
            name: `pkg-b`,
            version: `1.0.0`,
          });

          await run(`version`, `patch`, `--deferred`, {
            cwd: pkgB,
            env,
          });

          await expect(xfs.readdirPromise(rootDeferredFolder)).resolves.toHaveLength(1);
          await expect(xfs.existsPromise(workspaceDeferredFolder)).resolves.toEqual(false);

          await run(`version`, `apply`, `--all`, {
            env,
          });

          await expect(xfs.readJsonPromise(ppath.join(pkgA, Filename.manifest))).resolves.toMatchObject({
            version: `1.0.0`,
          });

          await expect(xfs.readJsonPromise(ppath.join(pkgB, Filename.manifest))).resolves.toMatchObject({
            version: `1.0.1`,
          });
        },
      ),
    );

    test(
      `it should apply the new version to multiple packages if needed`,
      makeTemporaryEnv(
        {
          private: true,
          workspaces: [
            `packages/*`,
          ],
        },
        async ({path, run}) => {
          const pkgA = ppath.join(path, `packages/pkg-a`);
          const pkgB = ppath.join(path, `packages/pkg-b`);

          await xfs.mkdirpPromise(pkgA);
          await xfs.mkdirpPromise(pkgB);

          await xfs.writeJsonPromise(ppath.join(pkgA, Filename.manifest), {
            name: `pkg-a`,
            version: `1.0.0`,
          });

          await xfs.writeJsonPromise(ppath.join(pkgB, Filename.manifest), {
            name: `pkg-b`,
            version: `1.0.0`,
          });

          await run(`version`, `patch`, `--deferred`, {
            cwd: pkgA,
          });

          await run(`version`, `patch`, `--deferred`, {
            cwd: pkgB,
          });

          await run(`version`, `apply`, `--all`);

          await expect(xfs.readJsonPromise(ppath.join(pkgA, Filename.manifest))).resolves.toMatchObject({
            version: `1.0.1`,
          });

          await expect(xfs.readJsonPromise(ppath.join(pkgB, Filename.manifest))).resolves.toMatchObject({
            version: `1.0.1`,
          });
        },
      ),
    );

    test(
      `it should apply "decline"`,
      makeTemporaryEnv(
        {
          version: `0.0.0`,
        },
        async ({path, run}) => {
          await run(`version`, `decline`, `--deferred`);

          await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
            version: `0.0.0`,
          });

          await run(`version`, `apply`);

          await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
            version: `0.0.0`,
          });
        },
      ),
    );

    test(
      `it should successfully apply a version bump that can't be described by a strategy (deferred)`,
      makeTemporaryEnv(
        {
          version: `1.0.0`,
        },
        async ({path, run}) => {
          await run(`version`, `3.4.5`, `--deferred`);

          await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
            version: `1.0.0`,
          });

          await run(`version`, `apply`);

          await expect(xfs.readJsonPromise(ppath.join(path, Filename.manifest))).resolves.toMatchObject({
            version: `3.4.5`,
          });
        },
      ),
    );
  });
});
