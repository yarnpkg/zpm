import {xfs} from '@yarnpkg/fslib';

describe(`Protocols`, () => {
  describe(`exec:`, () => {
    test(
      `it should execute a script to generate the package content`,
      makeTemporaryEnv({
        dependencies: {
          [`dynamic-pkg`]: `exec:./genpkg.js`,
        },
      }, async ({path, run, source}) => {
        await xfs.writeFilePromise(`${path}/genpkg.js`, `
          const {buildDir} = execEnv;
          fs.writeFileSync(path.join(buildDir, 'index.js'), 'module.exports = 42;');
          fs.writeFileSync(path.join(buildDir, 'package.json'), '{}');
        `);

        await run(`install`);

        await expect(source(`require('dynamic-pkg')`)).resolves.toEqual(42);
      }),
    );

    test(
      `it should correctly inject the built-in modules as global variables`,
      makeTemporaryEnv({
        dependencies: {
          [`dynamic-pkg`]: `exec:./genpkg.js`,
        },
      }, async ({path, run, source}) => {
        await xfs.writeFilePromise(`${path}/genpkg.js`, `
          const {buildDir} = execEnv;
          fs.writeFileSync(path.join(buildDir, 'index.js'), \`module.exports = \${JSON.stringify(Object.getOwnPropertyNames(global))};\`);
          fs.writeFileSync(path.join(buildDir, 'package.json'), '{}');
        `);

        await run(`install`);

        await expect(source(`require('dynamic-pkg')`)).resolves.toEqual(
          expect.arrayContaining(
            require(`module`).builtinModules.filter(name => name !== `module` && !name.startsWith(`_`)).concat([`Module`]),
          ),
        );
      }),
    );

    test(
      `it should correctly inject the \`execEnv\` global variable`,
      makeTemporaryEnv({
        dependencies: {
          [`dynamic-pkg`]: `exec:./genpkg.js`,
        },
      }, async ({path, run, source}) => {
        await xfs.writeFilePromise(`${path}/genpkg.js`, `
          const {buildDir} = execEnv;
          fs.writeFileSync(path.join(buildDir, 'index.js'), \`module.exports = \${JSON.stringify(execEnv)};\`);
          fs.writeFileSync(path.join(buildDir, 'package.json'), '{}');
        `);

        await run(`install`);

        await expect(source(`require('dynamic-pkg')`)).resolves.toMatchObject({
          tempDir: expect.any(String),
          buildDir: expect.any(String),
          locator: expect.any(String),
        });
      }),
    );

    test(
      `it should update the cache`,
      makeTemporaryEnv({
        dependencies: {
          [`dynamic-pkg`]: `exec:./genpkg.js`,
        },
      }, async ({path, run, source}) => {
        await xfs.writeFilePromise(`${path}/genpkg.js`, `
          const {buildDir} = execEnv;
          fs.writeFileSync(path.join(buildDir, 'index.js'), 'module.exports = 42;');
          fs.writeFileSync(path.join(buildDir, 'package.json'), '{}');
        `);

        await run(`install`);

        await xfs.writeFilePromise(`${path}/genpkg.js`, `
          const {buildDir} = execEnv;
          fs.writeFileSync(path.join(buildDir, 'index.js'), 'module.exports = 100;');
          fs.writeFileSync(path.join(buildDir, 'package.json'), '{}');
        `);

        await run(`install`);

        await expect(source(`require('dynamic-pkg')`)).resolves.toEqual(100);
      }),
    );

    test(
      `it should reuse a cache entry when the cache is immutable`,
      makeTemporaryEnv({
        dependencies: {
          [`dynamic-pkg`]: `exec:./genpkg.js`,
        },
      }, async ({path, run, source}) => {
        await xfs.writeFilePromise(`${path}/genpkg.js`, `
          const {buildDir} = execEnv;
          fs.writeFileSync(path.join(buildDir, 'index.js'), 'module.exports = 42;');
          fs.writeFileSync(path.join(buildDir, 'package.json'), '{}');
        `);

        await run(`install`);
        await run(`install`, `--immutable-cache`, {
          env: {
            YARN_ENABLE_SCRIPTS: `false`,
          },
        });

        await expect(source(`require('dynamic-pkg')`)).resolves.toEqual(42);
      }),
    );

    test(
      `it should honor enableScripts when generating packages`,
      makeTemporaryEnv({
        dependencies: {
          [`dynamic-pkg`]: `exec:./genpkg.js`,
        },
      }, async ({path, run}) => {
        await xfs.writeFilePromise(`${path}/.yarnrc.yml`, JSON.stringify({
          enableScripts: false,
        }));
        await xfs.writeFilePromise(`${path}/genpkg.js`, `
          const {buildDir} = execEnv;
          fs.writeFileSync(path.join(buildDir, 'index.js'), 'module.exports = 42;');
          fs.writeFileSync(path.join(buildDir, 'package.json'), '{}');
        `);

        await expect(run(`install`, {
          env: {
            YARN_ENABLE_SCRIPTS: `false`,
          },
        })).rejects.toThrow(/can't be built with the exec: protocol because all scripts have been disabled/);
      }),
    );

    test(
      `it should allow dependenciesMeta built to override enableScripts`,
      makeTemporaryEnv({
        dependencies: {
          [`dynamic-pkg`]: `exec:./genpkg.js`,
        },
        dependenciesMeta: {
          [`dynamic-pkg`]: {
            built: true,
          },
        },
      }, async ({path, run, source}) => {
        await xfs.writeFilePromise(`${path}/.yarnrc.yml`, JSON.stringify({
          enableScripts: false,
        }));
        await xfs.writeFilePromise(`${path}/genpkg.js`, `
          const {buildDir} = execEnv;
          fs.writeFileSync(path.join(buildDir, 'index.js'), 'module.exports = 42;');
          fs.writeFileSync(path.join(buildDir, 'package.json'), '{}');
        `);

        await run(`install`, {
          env: {
            YARN_ENABLE_SCRIPTS: `false`,
          },
        });

        await expect(source(`require('dynamic-pkg')`)).resolves.toEqual(42);
      }),
    );

    test(
      `it should reject exec dependencies from non-workspace packages`,
      makeTemporaryEnv({
        dependencies: {
          [`parent-pkg`]: `file:./parent`,
        },
      }, async ({path, run}) => {
        await xfs.mkdirPromise(`${path}/parent`);
        await xfs.writeJsonPromise(`${path}/parent/package.json`, {
          name: `parent-pkg`,
          version: `1.0.0`,
          dependencies: {
            [`dynamic-pkg`]: `exec:./genpkg.js`,
          },
        });
        await xfs.writeFilePromise(`${path}/parent/genpkg.js`, `
          const {buildDir} = execEnv;
          fs.writeFileSync(path.join(buildDir, 'index.js'), 'module.exports = 42;');
          fs.writeFileSync(path.join(buildDir, 'package.json'), '{}');
        `);

        await expect(run(`install`)).rejects.toThrow(/only workspaces can depend on exec: packages/);
      }),
    );
  });
});
