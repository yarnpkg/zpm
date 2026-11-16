import {PortablePath, npath, ppath, xfs} from '@yarnpkg/fslib';

// The test environment sets YARN_GLOBAL_FOLDER; clearing it lets us test the
// default. HOME (and USERPROFILE) is the parent of the project folder, and
// LOCALAPPDATA isn't set.
const defaultEnv = {
  YARN_GLOBAL_FOLDER: undefined,
  YARN_ENABLE_GLOBAL_CACHE: `true`,
};

// On Windows, Node always forwards USERPROFILE to child processes when it's
// missing from their environment, so we set the variables to empty values,
// which Yarn ignores.
const noHomeEnv = process.platform === `win32` ? {
  HOME: ``,
  USERPROFILE: ``,
} : {
  HOME: undefined,
  USERPROFILE: undefined,
};

// Yarn prints Windows paths with forward slashes, so we compare portable paths
const getGlobalFolder = async (run: any, env: Record<string, string | undefined>) => {
  const {stdout} = await run(`config`, `get`, `--json`, `globalFolder`, {env});
  return npath.toPortablePath(JSON.parse(stdout));
};

// The platform-specific folder when neither LOCALAPPDATA nor XDG_DATA_HOME is set
const getPlatformFolder = (home: PortablePath) => process.platform === `win32`
  ? ppath.join(home, `AppData/Local/Yarn/zpm`)
  : ppath.join(home, `.yarn/zpm`);

describe(`Features`, () => {
  describe(`globalFolder`, () => {
    test(`it should default to ~/.yarn/zpm (~/AppData/Local/Yarn/zpm on Windows)`, makeTemporaryEnv({
      dependencies: {
        [`no-deps`]: `1.0.0`,
      },
    }, async ({path, run}) => {
      const home = ppath.dirname(path);
      const globalFolder = getPlatformFolder(home);

      await expect(getGlobalFolder(run, defaultEnv)).resolves.toEqual(globalFolder);

      await run(`install`, {env: defaultEnv});

      const cache = await xfs.readdirPromise(ppath.join(globalFolder, `cache`));
      expect(cache.find(entry => entry.includes(`no-deps-npm-1.0.0`))).toBeDefined();
    }));

    test(`it should honour LOCALAPPDATA on Windows`, makeTemporaryEnv({
    }, async ({path, run}) => {
      const home = ppath.dirname(path);
      const localAppData = ppath.join(home, `local-app-data`);

      const env = {...defaultEnv, LOCALAPPDATA: npath.fromPortablePath(localAppData)};

      await expect(getGlobalFolder(run, env)).resolves.toEqual(process.platform === `win32`
        ? ppath.join(localAppData, `Yarn/zpm`)
        : getPlatformFolder(home));
    }));

    test(`it should honour XDG_DATA_HOME (except on Windows)`, makeTemporaryEnv({
      dependencies: {
        [`no-deps`]: `1.0.0`,
      },
    }, async ({path, run}) => {
      const home = ppath.dirname(path);
      const xdgDataHome = ppath.join(home, `xdg-data`);

      const env = {...defaultEnv, XDG_DATA_HOME: npath.fromPortablePath(xdgDataHome)};

      const globalFolder = process.platform === `win32`
        ? getPlatformFolder(home)
        : ppath.join(xdgDataHome, `yarn/zpm`);

      await expect(getGlobalFolder(run, env)).resolves.toEqual(globalFolder);

      await run(`install`, {env});

      const cache = await xfs.readdirPromise(ppath.join(globalFolder, `cache`));
      expect(cache.find(entry => entry.includes(`no-deps-npm-1.0.0`))).toBeDefined();

      await expect(xfs.existsPromise(ppath.join(home, `.yarn/zpm`))).resolves.toEqual(false);
    }));

    test(`it should keep using ~/.yarn/zpm if it exists and the platform-specific folder doesn't`, makeTemporaryEnv({
    }, async ({path, run}) => {
      const home = ppath.dirname(path);
      const xdgDataHome = ppath.join(home, `xdg-data`);

      const env = {...defaultEnv, XDG_DATA_HOME: npath.fromPortablePath(xdgDataHome)};

      const legacyFolder = ppath.join(home, `.yarn/zpm`);
      const platformFolder = process.platform === `win32`
        ? getPlatformFolder(home)
        : ppath.join(xdgDataHome, `yarn/zpm`);

      await xfs.mkdirPromise(legacyFolder, {recursive: true});

      await expect(getGlobalFolder(run, env)).resolves.toEqual(legacyFolder);

      await xfs.mkdirPromise(platformFolder, {recursive: true});

      await expect(getGlobalFolder(run, env)).resolves.toEqual(platformFolder);
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
