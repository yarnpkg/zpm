import {npath, ppath, xfs} from '@yarnpkg/fslib';

// The test environment sets YARN_GLOBAL_FOLDER; clearing it lets us test the
// default. HOME is the parent of the project folder.
const defaultEnv = {
  YARN_GLOBAL_FOLDER: undefined,
  YARN_ENABLE_GLOBAL_CACHE: `true`,
};

const noHomeEnv = {
  HOME: undefined,
  USERPROFILE: undefined,
};

describe(`Features`, () => {
  describe(`globalFolder`, () => {
    test(`it should default to ~/.yarn/zpm`, makeTemporaryEnv({
      dependencies: {
        [`no-deps`]: `1.0.0`,
      },
    }, async ({path, run}) => {
      const home = ppath.dirname(path);

      await expect(run(`config`, `get`, `--json`, `globalFolder`, {env: defaultEnv})).resolves.toMatchObject({
        stdout: `${JSON.stringify(npath.fromPortablePath(ppath.join(home, `.yarn/zpm`)))}\n`,
      });

      await run(`install`, {env: defaultEnv});

      const cache = await xfs.readdirPromise(ppath.join(home, `.yarn/zpm/cache`));
      expect(cache.find(entry => entry.includes(`no-deps-npm-1.0.0`))).toBeDefined();
    }));

    test(`it should honour XDG_DATA_HOME`, makeTemporaryEnv({
      dependencies: {
        [`no-deps`]: `1.0.0`,
      },
    }, async ({path, run}) => {
      const home = ppath.dirname(path);
      const xdgDataHome = ppath.join(home, `xdg-data`);

      const env = {...defaultEnv, XDG_DATA_HOME: npath.fromPortablePath(xdgDataHome)};

      await expect(run(`config`, `get`, `--json`, `globalFolder`, {env})).resolves.toMatchObject({
        stdout: `${JSON.stringify(npath.fromPortablePath(ppath.join(xdgDataHome, `yarn/zpm`)))}\n`,
      });

      await run(`install`, {env});

      const cache = await xfs.readdirPromise(ppath.join(xdgDataHome, `yarn/zpm/cache`));
      expect(cache.find(entry => entry.includes(`no-deps-npm-1.0.0`))).toBeDefined();

      await expect(xfs.existsPromise(ppath.join(home, `.yarn/zpm`))).resolves.toEqual(false);
    }));

    test(`it should keep using ~/.yarn/zpm if it exists and the XDG_DATA_HOME folder doesn't`, makeTemporaryEnv({
    }, async ({path, run}) => {
      const home = ppath.dirname(path);
      const xdgDataHome = ppath.join(home, `xdg-data`);

      const env = {...defaultEnv, XDG_DATA_HOME: npath.fromPortablePath(xdgDataHome)};

      await xfs.mkdirPromise(ppath.join(home, `.yarn/zpm`), {recursive: true});

      await expect(run(`config`, `get`, `--json`, `globalFolder`, {env})).resolves.toMatchObject({
        stdout: `${JSON.stringify(npath.fromPortablePath(ppath.join(home, `.yarn/zpm`)))}\n`,
      });

      await xfs.mkdirPromise(ppath.join(xdgDataHome, `yarn/zpm`), {recursive: true});

      await expect(run(`config`, `get`, `--json`, `globalFolder`, {env})).resolves.toMatchObject({
        stdout: `${JSON.stringify(npath.fromPortablePath(ppath.join(xdgDataHome, `yarn/zpm`)))}\n`,
      });
    }));

    test(`it should work without a home folder when globalFolder is set`, makeTemporaryEnv({
      dependencies: {
        [`no-deps`]: `1.0.0`,
      },
      scripts: {
        hello: `echo hello world`,
      },
    }, async ({path, run}) => {
      const globalFolder = ppath.join(path, `global`);

      const env = {...defaultEnv, ...noHomeEnv, YARN_GLOBAL_FOLDER: npath.fromPortablePath(globalFolder)};

      await run(`install`, {env});

      const cache = await xfs.readdirPromise(ppath.join(globalFolder, `cache`));
      expect(cache.find(entry => entry.includes(`no-deps-npm-1.0.0`))).toBeDefined();

      await expect(run(`run`, `hello`, {env})).resolves.toMatchObject({
        stdout: `hello world\n`,
      });
    }));

    test(`it should report a clear error without a home folder when globalFolder isn't set`, makeTemporaryEnv({
      dependencies: {
        [`no-deps`]: `1.0.0`,
      },
    }, async ({path, run}) => {
      const env = {...defaultEnv, ...noHomeEnv};

      await expect(run(`install`, {env})).rejects.toMatchObject({
        code: 1,
        stdout: expect.stringContaining(`set the globalFolder setting (or the YARN_GLOBAL_FOLDER environment variable)`),
      });
    }));
  });
});
