import {Filename, ppath, xfs} from '@yarnpkg/fslib';

describe(`Commands`, () => {
  describe(`init`, () => {
    test(
      `it should create a new package.json in the local directory if it doesn't exist`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await xfs.mktempPromise(async tmpDir => {
          const pkgDir = ppath.join(tmpDir, `my-package`);
          await xfs.mkdirpPromise(pkgDir);

          await run(`init`, {
            cwd: pkgDir,
          });

          await expect(xfs.readJsonPromise(ppath.join(pkgDir, Filename.manifest))).resolves.toMatchObject({
            name: `my-package`,
          });
        });
      }),
    );

    test(
      `it should create a new package.json in the specified directory if it doesn't exist`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await xfs.mktempPromise(async tmpDir => {
          const pkgDir = ppath.join(tmpDir, `my-package`);
          await xfs.mkdirpPromise(pkgDir);

          await run(`./my-package`, `init`, {
            cwd: tmpDir,
          });

          await expect(xfs.readJsonPromise(ppath.join(pkgDir, Filename.manifest))).resolves.toMatchObject({
            name: `my-package`,
          });
        });
      }),
    );

    test(
      `it should create a new package.json in the specified directory even if said directory doesn't exist`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await xfs.mktempPromise(async tmpDir => {
          const pkgDir = ppath.join(tmpDir, `my-package`);

          await run(`./my-package`, `init`, {
            cwd: tmpDir,
          });

          await expect(xfs.readJsonPromise(ppath.join(pkgDir, Filename.manifest))).resolves.toMatchObject({
            name: `my-package`,
          });
        });
      }),
    );

    test(
      `it shouldn't mark the package as private by default`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await xfs.mktempPromise(async tmpDir => {
          const pkgDir = ppath.join(tmpDir, `my-package`);
          await xfs.mkdirpPromise(pkgDir);

          await run(`init`, {
            cwd: pkgDir,
          });

          const manifest = await xfs.readJsonPromise(ppath.join(pkgDir, Filename.manifest));
          expect(manifest).not.toHaveProperty(`private`);
          expect(manifest).not.toHaveProperty(`workspaces`);
        });
      }),
    );

    test(
      `it should mark the package as private when using -p,--private`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await xfs.mktempPromise(async tmpDir => {
          const pkgDir = ppath.join(tmpDir, `my-package`);
          await xfs.mkdirpPromise(pkgDir);

          await run(`init`, `-p`, {
            cwd: pkgDir,
          });

          await expect(xfs.readJsonPromise(ppath.join(pkgDir, Filename.manifest))).resolves.toMatchObject({
            name: `my-package`,
            private: true,
          });
        });
      }),
    );

    test(
      `it should use the name given with -n,--name`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await xfs.mktempPromise(async tmpDir => {
          const pkgDir = ppath.join(tmpDir, `my-package`);
          await xfs.mkdirpPromise(pkgDir);

          await run(`init`, `-n`, `other-name`, {
            cwd: pkgDir,
          });

          await expect(xfs.readJsonPromise(ppath.join(pkgDir, Filename.manifest))).resolves.toMatchObject({
            name: `other-name`,
          });
        });
      }),
    );

    test(
      `it should create a private workspace root when using -w,--workspace`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await xfs.mktempPromise(async tmpDir => {
          const pkgDir = ppath.join(tmpDir, `my-package`);
          await xfs.mkdirpPromise(pkgDir);

          await run(`init`, `-w`, {
            cwd: pkgDir,
          });

          await expect(xfs.readJsonPromise(ppath.join(pkgDir, Filename.manifest))).resolves.toMatchObject({
            name: `my-package`,
            private: true,
            workspaces: [`packages/*`],
          });

          expect(xfs.existsSync(ppath.join(pkgDir, `packages`))).toEqual(true);
        });
      }),
    );

    test(
      `it shouldn't override the workspaces of an existing workspace root when using -w,--workspace`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await xfs.mktempPromise(async tmpDir => {
          const pkgDir = ppath.join(tmpDir, `my-package`);
          await xfs.mkdirpPromise(pkgDir);

          await xfs.writeJsonPromise(ppath.join(pkgDir, Filename.manifest), {
            name: `my-package`,
            workspaces: [`apps/*`],
          });

          await run(`init`, `-w`, {
            cwd: pkgDir,
          });

          const manifest = await xfs.readJsonPromise(ppath.join(pkgDir, Filename.manifest));
          expect(manifest).toMatchObject({
            name: `my-package`,
            workspaces: [`apps/*`],
          });

          // Like Yarn 4, an existing package isn't made private implicitly
          expect(manifest).not.toHaveProperty(`private`);
          expect(xfs.existsSync(ppath.join(pkgDir, `packages`))).toEqual(false);
        });
      }),
    );
  });
});
