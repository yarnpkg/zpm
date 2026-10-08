import {ppath, xfs} from '@yarnpkg/fslib';

async function getCacheContent(cacheFolder) {
  const cacheContent = (await xfs.readdirPromise(cacheFolder))
    .filter(file => !file.startsWith(`.`));

  cacheContent.sort();

  return cacheContent;
}

describe(`Commands`, () => {
  describe(`workspaces focus`, () => {
    test(
      `should install the dependencies for the focused workspace only`,
      makeTemporaryEnv(
        {
          private: true,
          workspaces: [`packages/*`],
        },
        async ({path, run, source}) => {
          await setupProject(path, source);

          await run(`install`);

          const cacheFolder = ppath.join(path, `.yarn/cache`);
          await xfs.removePromise(cacheFolder);

          await run(`workspaces`, `focus`, {
            cwd: ppath.join(path, `packages/foo`),
          });

          await expect(getCacheContent(cacheFolder)).resolves.toEqual([
            expect.stringContaining(`no-deps-npm-1.0.0-`),
          ]);

          await expect(source(`require('no-deps')`, {
            cwd: ppath.join(path, `packages/foo`),
          })).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `should install the dependencies for specified workspaces only`,
      makeTemporaryEnv(
        {
          private: true,
          workspaces: [`packages/*`],
        },
        async ({path, run, source}) => {
          await setupProject(path);

          await run(`install`);

          const cacheFolder = ppath.join(path, `.yarn/cache`);
          await xfs.removePromise(cacheFolder);

          await run(`workspaces`, `focus`, `foo`, `bar`, {
            cwd: ppath.join(path, `packages/foo`),
          });

          await expect(getCacheContent(cacheFolder)).resolves.toEqual([
            expect.stringContaining(`no-deps-npm-1.0.0-`),
            expect.stringContaining(`no-deps-npm-2.0.0-`),
          ]);

          await expect(source(`require('no-deps')`, {
            cwd: ppath.join(path, `packages/foo`),
          })).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });

          await expect(source(`require('no-deps')`, {
            cwd: ppath.join(path, `packages/bar`),
          })).resolves.toMatchObject({
            name: `no-deps`,
            version: `2.0.0`,
          });
        },
      ),
    );

    test(
      `should follow local workspace dependencies`,
      makeTemporaryEnv(
        {
          private: true,
          workspaces: [`packages/*`],
        },
        async ({path, run, source}) => {
          await setupProject(path);

          await run(`install`);

          const cacheFolder = ppath.join(path, `.yarn/cache`);
          await xfs.removePromise(cacheFolder);

          await run(`workspaces`, `focus`, {
            cwd: ppath.join(path, `packages/baz`),
          });

          await expect(getCacheContent(cacheFolder)).resolves.toEqual([
            expect.stringContaining(`no-deps-npm-2.0.0-`),
          ]);

          await expect(source(`require('no-deps')`, {
            cwd: ppath.join(path, `packages/bar`),
          })).resolves.toMatchObject({
            name: `no-deps`,
            version: `2.0.0`,
          });
        },
      ),
    );

    test(
      `should follow local workspace devDependencies`,
      makeTemporaryEnv(
        {
          private: true,
          workspaces: [`packages/*`],
        },
        async ({path, run, source}) => {
          await setupProject(path);

          await run(`workspaces`, `focus`, {
            cwd: ppath.join(path, `packages/quux`),
          });

          const cacheFolder = ppath.join(path, `.yarn/cache`);
          await expect(getCacheContent(cacheFolder)).resolves.toEqual([
            expect.stringContaining(`no-deps-npm-1.0.0-`),
            expect.stringContaining(`no-deps-npm-2.0.0-`),
          ]);

          await expect(source(`require('no-deps')`, {
            cwd: ppath.join(path, `packages/quux`),
          })).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `should not follow local workspace devDependencies for production installs`,
      makeTemporaryEnv(
        {
          private: true,
          workspaces: [`packages/*`],
        },
        async ({path, run, source}) => {
          await setupProject(path);

          await run(`workspaces`, `focus`, `quux`, `--production`, {
            cwd: path,
          });

          const cacheFolder = ppath.join(path, `.yarn/cache`);
          await expect(getCacheContent(cacheFolder)).resolves.toEqual([
            expect.stringContaining(`no-deps-npm-1.0.0-`),
          ]);
        },
      ),
    );

    test(
      `should install development dependencies by default`,
      makeTemporaryEnv(
        {
          private: true,
          workspaces: [`packages/*`],
        },
        async ({path, run, source}) => {
          await setupProject(path);

          await run(`install`);

          const cacheFolder = ppath.join(path, `.yarn/cache`);
          await xfs.removePromise(cacheFolder);

          await run(`workspaces`, `focus`, `qux`, {
            cwd: path,
          });

          await expect(getCacheContent(cacheFolder)).resolves.toEqual([
            expect.stringContaining(`no-deps-bins-npm-1.0.0-`),
            expect.stringContaining(`no-deps-npm-1.0.0-`),
          ]);

          await expect(source(`require('no-deps-bins')`, {
            cwd: ppath.join(path, `packages/qux`),
          })).resolves.toMatchObject({
            name: `no-deps-bins`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `should only install production dependencies if requested`,
      makeTemporaryEnv(
        {
          private: true,
          workspaces: [`packages/*`],
        },
        async ({path, run}) => {
          await setupProject(path);

          await run(`install`);

          const cacheFolder = ppath.join(path, `.yarn/cache`);
          await xfs.removePromise(cacheFolder);

          await run(`workspaces`, `focus`, `qux`, `--production`, {
            cwd: path,
          });

          await expect(getCacheContent(cacheFolder)).resolves.toEqual([
            expect.stringContaining(`no-deps-npm-1.0.0-`),
          ]);
        },
      ),
    );

    test(
      `should not execute postinstall scripts of unspecified workspace`,
      makeTemporaryEnv(
        {
          private: true,
          workspaces: [`packages/*`],
        },
        async ({path, run}) => {
          await setupProject(path);

          await run(`workspaces`, `focus`, `foo`, `bar`, {
            cwd: ppath.join(path, `packages/foo`),
          });

          await expect(xfs.existsSync(ppath.join(path, `packages/foo/postinstall.log`))).toBeTruthy();
          await expect(xfs.existsSync(ppath.join(path, `packages/qux/postinstall.log`))).toBeFalsy();
        },
      ),
    );

    test(
      `should not run the build scripts with --mode=skip-build`,
      makeTemporaryEnv(
        {
          private: true,
          workspaces: [`packages/*`],
        },
        async ({path, run}) => {
          await setupProject(path);

          await run(`workspaces`, `focus`, `foo`, `--mode=skip-build`);

          await expect(xfs.existsSync(ppath.join(path, `packages/foo/postinstall.log`))).toBeFalsy();
        },
      ),
    );

    test(
      `should refuse to modify the lockfile with --immutable`,
      makeTemporaryEnv(
        {
          private: true,
          workspaces: [`packages/*`],
        },
        async ({path, run}) => {
          await setupProject(path);

          await run(`install`);
          await run(`workspaces`, `focus`, `foo`, `--immutable`);

          await xfs.writeJsonPromise(ppath.join(path, `packages/foo/package.json`), {
            name: `foo`,
            dependencies: {[`one-fixed-dep`]: `1.0.0`},
          });

          await expect(run(`workspaces`, `focus`, `foo`, `--immutable`)).rejects.toMatchObject({
            code: 1,
          });
        },
      ),
    );

    test(
      `should reject --mode=update-lockfile`,
      makeTemporaryEnv(
        {
          private: true,
          workspaces: [`packages/*`],
        },
        async ({path, run}) => {
          await setupProject(path);

          await expect(run(`workspaces`, `focus`, `foo`, `--mode=update-lockfile`)).rejects.toMatchObject({
            code: 1,
            stdout: expect.stringContaining(`use yarn install instead`),
          });
        },
      ),
    );

    test(
      `should focus a workspace that doesn't depend on the root with the pnpm linker`,
      makeTemporaryEnv(
        {
          private: true,
          workspaces: [`packages/*`],
        },
        {
          nodeLinker: `pnpm`,
        },
        async ({path, run}) => {
          await setupProject(path);

          await run(`workspaces`, `focus`, `foo`, {
            cwd: ppath.join(path, `packages/foo`),
          });

          await expect(xfs.readJsonPromise(ppath.join(path, `packages/foo/node_modules/no-deps/package.json`))).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `should run scripts after a focused pnpm-linker install without a lockfile`,
      makeTemporaryEnv(
        {
          private: true,
          workspaces: [`packages/*`],
        },
        {
          nodeLinker: `pnpm`,
        },
        async ({path, run}) => {
          await setupProject(path);
          await xfs.writeJsonPromise(ppath.join(path, `packages/bar/package.json`), {
            name: `bar`,
            dependencies: {[`no-deps`]: `2.0.0`},
            scripts: {hello: `echo hello`},
          });

          // Our own linker creates node_modules/.pnpm; it mustn't be mistaken
          // for a pnpm install to import
          await run(`workspaces`, `focus`, `bar`);

          await expect(xfs.existsSync(ppath.join(path, `yarn.lock`))).toBeFalsy();
          await expect(run(`workspace`, `bar`, `run`, `hello`)).resolves.toMatchObject({
            stdout: expect.stringContaining(`hello`),
          });
        },
      ),
    );
  });
});

async function setupProject(path) {
  const pkg = async (name, dependencies, devDependencies, scripts) => {
    await xfs.mkdirpPromise(ppath.join(path, `packages/${name}`));
    await xfs.writeJsonPromise(ppath.join(path, `packages/${name}/package.json`), {name, dependencies, devDependencies, scripts});
  };

  await pkg(`foo`, {[`no-deps`]: `1.0.0`}, {}, {postinstall: `echo 'postinstall' > postinstall.log`});
  await pkg(`bar`, {[`no-deps`]: `2.0.0`});
  await pkg(`baz`, {[`bar`]: `workspace:*`});
  await pkg(`qux`, {[`no-deps`]: `1.0.0`}, {[`no-deps-bins`]: `1.0.0`}, {postinstall: `echo 'postinstall' > postinstall.log`});
  await pkg(`quux`, {[`no-deps`]: `1.0.0`}, {[`bar`]: `workspace:*`});
}
