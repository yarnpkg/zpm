import {xfs, ppath, PortablePath, Filename} from '@yarnpkg/fslib';

describe(`Commands`, () => {
  describe(`set version`, () => {
    test(
      `it should set the packageManager field to the requested semver version`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`set`, `version`, `3.0.0`);
        await check(path, {corepackVersion: `3.0.0`});
      }),
    );

    // The following tests resolve ranges against the list of releases
    // published on repo.yarnpkg.com, so they need network access to it

    test.each([`4`, `4.x`, `4.*`, `^4.1`])(
      `it should set the packageManager field to the highest release matching %s`,
      selector => makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`set`, `version`, selector);

        const {packageManager} = await xfs.readJsonPromise(ppath.join(path, Filename.manifest));
        const [, major, minor] = packageManager.match(/^yarn@(\d+)\.(\d+)\.(\d+)$/) ?? [];

        expect(major).toEqual(`4`);
        // 4.18.1 was already released when this test was written
        expect(Number(minor)).toBeGreaterThanOrEqual(18);
      })(),
    );

    test(
      `it should set the packageManager field to the highest release matching a partial version`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        await run(`set`, `version`, `4.1`);
        await check(path, {corepackVersion: `4.1.1`});
      }),
    );
  });
});

async function check(path: PortablePath, checks: {corepackVersion: string | RegExp}) {
  const manifestPath = ppath.join(path, Filename.manifest);

  await expect(xfs.readJsonPromise(manifestPath)).resolves.toMatchObject({
    packageManager: checks.corepackVersion instanceof RegExp
      ? expect.stringMatching(`yarn@${checks.corepackVersion.source}`)
      : `yarn@${checks.corepackVersion}`,
  });
}
