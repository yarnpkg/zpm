import {xfs} from '@yarnpkg/fslib';

describe(`autoClean`, () => {
  test(`it should remove inactive entries from the cache`, makeTemporaryEnv({
    dependencies: {
      [`one-fixed-dep`]: `1.0.0`,
    },
  }, async ({path, run, source}) => {
    await run(`install`);
    const fileCount1 = (await xfs.readdirPromise(`${path}/.yarn/cache`)).length;

    await run(`remove`, `one-fixed-dep`);
    const fileCount2 = (await xfs.readdirPromise(`${path}/.yarn/cache`)).length;

    expect(fileCount2).toEqual(fileCount1 - 2);
  }));

  test(`it shouldn't remove active entries from the cache`, makeTemporaryEnv({
    dependencies: {
      [`no-deps`]: `1.0.0`,
      [`one-fixed-dep`]: `1.0.0`,
    },
  }, async ({path, run, source}) => {
    await run(`install`);
    const fileCount1 = (await xfs.readdirPromise(`${path}/.yarn/cache`)).length;

    await run(`remove`, `one-fixed-dep`);
    const fileCount2 = (await xfs.readdirPromise(`${path}/.yarn/cache`)).length;

    expect(fileCount2).toEqual(fileCount1 - 1);
  }));

  test(`it shouldn't remove inactive entries from the cache when enableLocalCacheCleanup is false`, makeTemporaryEnv({
    dependencies: {
      [`one-fixed-dep`]: `1.0.0`,
    },
  }, {
    enableLocalCacheCleanup: false,
  }, async ({path, run, source}) => {
    await run(`install`);
    const cacheFiles1 = await xfs.readdirPromise(`${path}/.yarn/cache`);

    await run(`remove`, `one-fixed-dep`);
    const cacheFiles2 = await xfs.readdirPromise(`${path}/.yarn/cache`);

    expect(cacheFiles2.sort()).toEqual(cacheFiles1.sort());
  }));

  test(`it shouldn't remove inactive entries from a custom cacheFolder`, makeTemporaryEnv({
    dependencies: {
      [`one-fixed-dep`]: `1.0.0`,
    },
  }, async ({path, run, source}) => {
    await run(`config`, `set`, `cacheFolder`, `${path}/custom-cache`);

    await run(`install`);
    const cacheFiles1 = await xfs.readdirPromise(`${path}/custom-cache`);

    await run(`remove`, `one-fixed-dep`);
    const cacheFiles2 = await xfs.readdirPromise(`${path}/custom-cache`);

    expect(cacheFiles1.length).toBeGreaterThan(0);
    expect(cacheFiles2.sort()).toEqual(cacheFiles1.sort());
  }));

  test(`it should refuse to remove inactive entries from an immutable cache`, makeTemporaryEnv({
    dependencies: {
      [`one-fixed-dep`]: `1.0.0`,
    },
  }, async ({path, run, source}) => {
    await run(`install`);
    const cacheFiles1 = await xfs.readdirPromise(`${path}/.yarn/cache`);

    await xfs.writeJsonPromise(`${path}/package.json`, {});

    await expect(run(`install`, `--immutable-cache`)).rejects.toThrow();

    const cacheFiles2 = await xfs.readdirPromise(`${path}/.yarn/cache`);
    expect(cacheFiles2.sort()).toEqual(cacheFiles1.sort());
  }));

  test(`it shouldn't complain about inactive entries in an immutable cache when enableLocalCacheCleanup is false`, makeTemporaryEnv({
    dependencies: {
      [`one-fixed-dep`]: `1.0.0`,
    },
  }, {
    enableLocalCacheCleanup: false,
  }, async ({path, run, source}) => {
    await run(`install`);
    const cacheFiles1 = await xfs.readdirPromise(`${path}/.yarn/cache`);

    await xfs.writeJsonPromise(`${path}/package.json`, {});

    await run(`install`, `--immutable-cache`);

    const cacheFiles2 = await xfs.readdirPromise(`${path}/.yarn/cache`);
    expect(cacheFiles2.sort()).toEqual(cacheFiles1.sort());
  }));
});
