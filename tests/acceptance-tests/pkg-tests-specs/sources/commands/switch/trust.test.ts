import {Filename, ppath, xfs} from '@yarnpkg/fslib';

describe(`Commands`, () => {
  describe(`switch trust`, () => {
    test(
      `it should set and check project trust`,
      makeTemporaryEnv({}, async ({path, runSwitch}) => {
        await expect(runSwitch(`switch`, `trust`, `--check`, path)).rejects.toMatchObject({
          code: 3,
        });

        await expect(runSwitch(`switch`, `trust`, `--set`, `false`, path)).resolves.toMatchObject({
          code: 0,
        });

        await expect(runSwitch(`switch`, `trust`, `--check`, path)).rejects.toMatchObject({
          code: 2,
        });

        await expect(runSwitch(`switch`, `trust`, `--set`, `true`, path)).resolves.toMatchObject({
          code: 0,
        });

        await expect(runSwitch(`switch`, `trust`, `--check`, path)).resolves.toMatchObject({
          code: 0,
        });

        await expect(runSwitch(`switch`, `trust`, `--set`, `null`, path)).resolves.toMatchObject({
          code: 0,
        });

        await expect(runSwitch(`switch`, `trust`, `--check`, path)).rejects.toMatchObject({
          code: 3,
        });
      }),
    );

    test(
      `it should expose project trust through the cache listing`,
      makeTemporaryEnv({}, async ({path, runSwitch}) => {
        await runSwitch(`switch`, `trust`, `--set`, `true`, path);

        await expect(runSwitch(`switch`, `cache`)).resolves.toMatchObject({
          code: 0,
          stdout: expect.stringContaining(`Trusted: true`),
        });
      }),
    );

    test(
      `it should require trust before running install scripts through Yarn Switch`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps-scripted`]: `1.0.0`,
        },
      }, async ({path, runSwitch}) => {
        await expect(runSwitch(`install`)).rejects.toMatchObject({
          code: 1,
          stdout: expect.stringContaining(`must be trusted before Yarn can run install scripts`),
        });

        await runSwitch(`switch`, `trust`, `--set`, `true`, path);

        await expect(runSwitch(`install`)).resolves.toMatchObject({
          code: 0,
        });
      }),
    );

    test(
      `it should implicitly trust projects when running on CI`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps-scripted`]: `1.0.0`,
        },
      }, async ({path, runSwitch}) => {
        await expect(runSwitch(`switch`, `trust`, `--check`, path, {
          env: {CI: `1`},
        })).resolves.toMatchObject({
          code: 0,
        });

        await expect(runSwitch(`install`, {
          env: {CI: `1`},
        })).resolves.toMatchObject({
          code: 0,
        });
      }),
    );

    test(
      `it should still honor an explicit untrust on CI`,
      makeTemporaryEnv({}, async ({path, runSwitch}) => {
        await runSwitch(`switch`, `trust`, `--set`, `false`, path);

        await expect(runSwitch(`switch`, `trust`, `--check`, path, {
          env: {CI: `1`},
        })).rejects.toMatchObject({
          code: 2,
        });
      }),
    );

    test(
      `it should require trust before interpolating project configuration through Yarn Switch`,
      makeTemporaryEnv({}, async ({path, runSwitch}) => {
        await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
          initScope: `\${CONFIG_INIT_SCOPE}`,
        });

        await expect(runSwitch(`config`, `get`, `initScope`, {
          env: {CONFIG_INIT_SCOPE: `acme`},
        })).rejects.toMatchObject({
          code: 1,
          stdout: expect.stringContaining(`must be trusted before Yarn can interpolate project configuration`),
        });

        await runSwitch(`switch`, `trust`, `--set`, `true`, path);

        await expect(runSwitch(`config`, `get`, `initScope`, {
          env: {CONFIG_INIT_SCOPE: `acme`},
        })).resolves.toMatchObject({
          code: 0,
          stdout: `acme\n`,
        });
      }),
    );

    test(
      `it shouldn't require trust when project configuration values don't change during interpolation`,
      makeTemporaryEnv({}, async ({path, runSwitch}) => {
        await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
          initScope: `$`,
        });

        await expect(runSwitch(`config`, `get`, `initScope`)).resolves.toMatchObject({
          code: 0,
          stdout: `$\n`,
        });
      }),
    );

    test(
      `it shouldn't require trust for interpolating user configuration through Yarn Switch`,
      makeTemporaryEnv({}, async ({path, runSwitch}) => {
        await xfs.writeJsonPromise(ppath.join(ppath.dirname(path), Filename.rc), {
          initScope: `\${CONFIG_INIT_SCOPE}`,
        });

        await expect(runSwitch(`config`, `get`, `initScope`, {
          env: {CONFIG_INIT_SCOPE: `acme`},
        })).resolves.toMatchObject({
          code: 0,
          stdout: `acme\n`,
        });
      }),
    );
  });
});
