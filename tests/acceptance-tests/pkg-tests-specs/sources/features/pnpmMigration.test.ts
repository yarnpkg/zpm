import {ppath, xfs} from '@yarnpkg/fslib';

const {
  exec: {execFile},
  tests: {setPackageWhitelist, startPackageServer},
} = require(`pkg-tests-core`);

describe(`Features`, () => {
  describe(`Pnpm Migration`, () => {
    test(`it should correctly import resolutions from a pnpm node_modules`, makeTemporaryEnv({
      dependencies: {
        [`one-range-dep`]: `1.0.0`,
        [`one-scoped-range-dep`]: `1.0.0`,
      },
    }, async ({path, run, source}) => {
      const registryUrl = await startPackageServer();

      // Configure pnpm to use our test registry
      await xfs.writeFilePromise(ppath.join(path, `.npmrc`), `registry=${registryUrl}\n`);

      // First, install with pnpm when only 1.0.0 is available
      // This ensures that pnpm resolves no-deps@^1.0.0 to 1.0.0
      await setPackageWhitelist(new Map([
        [`@scoped/no-deps`, new Set([`1.0.0`])],
        [`no-deps`, new Set([`1.0.0`])],
      ]), async () => {
        await execFile(`pnpm`, [`install`], {
          cwd: path,
          env: {
            ...process.env,
            npm_config_registry: registryUrl,
          },
        });
      });

      await run(`install`);

      await expect(source(`require('one-scoped-range-dep')`)).resolves.toMatchObject({
        name: `one-scoped-range-dep`,
        version: `1.0.0`,
        dependencies: {
          [`@scoped/no-deps`]: {
            name: `@scoped/no-deps`,
            version: `1.0.0`, // shouldn't be 1.0.1 if pnpm migration works correctly
          },
        },
      });

      await expect(source(`require('one-range-dep')`)).resolves.toMatchObject({
        name: `one-range-dep`,
        version: `1.0.0`,
        dependencies: {
          [`no-deps`]: {
            name: `no-deps`,
            version: `1.0.0`, // shouldn't be 1.0.1 if pnpm migration works correctly
          },
        },
      });
    }));

    test(`it should correctly import resolutions from packages with non-conventional urls`, makeTemporaryEnv({
      dependencies: {
        [`unconventional-tarball`]: `*`,
      },
    }, async ({path, run, source}) => {
      const registryUrl = await startPackageServer();

      // Configure pnpm to use our test registry
      await xfs.writeFilePromise(ppath.join(path, `.npmrc`), `registry=${registryUrl}\n`);

      // First, install with pnpm when only 1.0.0 is available
      // This ensures that pnpm resolves unconventional-tarball@* to 1.0.0
      await setPackageWhitelist(new Map([
        [`unconventional-tarball`, new Set([`1.0.0`])],
      ]), async () => {
        await execFile(`pnpm`, [`install`], {
          cwd: path,
          env: {
            ...process.env,
            npm_config_registry: registryUrl,
          },
        });
      });

      await run(`install`);

      await expect(source(`require('unconventional-tarball')`)).resolves.toMatchObject({
        name: `unconventional-tarball`,
        version: `1.0.0`,
      });
    }));
  });
});
