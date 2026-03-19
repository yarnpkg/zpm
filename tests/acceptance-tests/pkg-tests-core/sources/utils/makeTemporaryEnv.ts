import {npath}     from '@yarnpkg/fslib';
import {delimiter} from 'path';

import * as exec   from './exec';
import * as tests  from './tests';

const {generatePkgDriver} = tests;
const {execFile} = exec;

const baseEnv = (nativePath: string, nativeHomePath: string, registryUrl: string, rcEnv: Record<string, any>, env?: Record<string, string | undefined>) => ({
  [`HOME`]: nativeHomePath,
  [`USERPROFILE`]: nativeHomePath,
  [`PATH`]: `${nativePath}/bin${delimiter}${process.env.PATH}`,
  [`RUST_BACKTRACE`]: `1`,
  [`YARN_IS_TEST_ENV`]: `true`,
  [`YARN_GLOBAL_FOLDER`]: `${nativePath}/.yarn/global`,
  [`YARN_NPM_REGISTRY_SERVER`]: registryUrl,
  [`YARN_UNSAFE_HTTP_WHITELIST`]: new URL(registryUrl).hostname,
  [`YARN_NODE_DIST_URL`]: `${registryUrl}/node/dist`,
  // Otherwise we'd send telemetry event when running tests
  [`YARN_ENABLE_TELEMETRY`]: `0`,
  // Otherwise snapshots relying on this would break each time it's bumped
  [`YARN_CACHE_VERSION_OVERRIDE`]: `0`,
  // Otherwise the output isn't stable between runs
  [`YARN_ENABLE_PROGRESS_BARS`]: `false`,
  [`YARN_ENABLE_TIMERS`]: `false`,
  [`FORCE_COLOR`]: `0`,
  // Otherwise the output wouldn't be the same on CI vs non-CI
  [`YARN_ENABLE_INLINE_BUILDS`]: `false`,
  // Otherwise we would more often test the fallback rather than the real logic
  [`YARN_PNP_FALLBACK_MODE`]: `none`,
  // Otherwise tests fail on systems where this is globally set to true
  [`YARN_ENABLE_GLOBAL_CACHE`]: `false`,
  // To make sure we can call Git commands
  [`GIT_AUTHOR_NAME`]: `John Doe`,
  [`GIT_AUTHOR_EMAIL`]: `john.doe@example.org`,
  [`GIT_COMMITTER_NAME`]: `John Doe`,
  [`GIT_COMMITTER_EMAIL`]: `john.doe@example.org`,
  // Older versions of Windows need this set to not have node throw an error
  [`NODE_SKIP_PLATFORM_CHECK`]: `1`,
  // We don't want the PnP runtime to be accidentally injected
  [`NODE_OPTIONS`]: ``,
  // Shorter warmup for faster tests (production default is 1s)
  [`YARN_DAEMON_DEFAULT_WARMUP_PERIOD`]: `500ms`,
  ...rcEnv,
  ...env,
});

const getYarnBinaryPath = () => {
  return process.env.TEST_BINARY
    ?? require.resolve(`${__dirname}/../../../../../target/release/yarn-bin`);
};

const mte = generatePkgDriver({
  getName() {
    return `yarn`;
  },
  getYarnBinary() {
    return getYarnBinaryPath();
  },
  async runDriver(
    path,
    [command, ...args],
    {cwd, execArgv = [], projectFolder, registryUrl, env, stdin, ...config},
  ) {
    const rcEnv: Record<string, any> = {};
    for (const [key, value] of Object.entries(config))
      rcEnv[`YARN_${key.replace(/([A-Z])/g, `_$1`).toUpperCase()}`] = Array.isArray(value) ? value.join(`,`) : value;

    const nativePath = npath.fromPortablePath(path);
    const nativeHomePath = npath.dirname(nativePath);

    const cwdArgs = typeof projectFolder !== `undefined`
      ? [projectFolder]
      : [];

    const yarnBinary = getYarnBinaryPath();

    const yarnBinaryArgs = yarnBinary.match(/\.[cm]?js$/)
      ? [process.execPath, yarnBinary]
      : [yarnBinary];

    const res = await execFile(yarnBinaryArgs[0]!, [...execArgv, ...yarnBinaryArgs.slice(1), ...cwdArgs, command, ...args], {
      cwd: cwd || path,
      stdin,
      env: {
        ...baseEnv(nativePath, nativeHomePath, registryUrl, rcEnv, env),
        [`YARNSW_DEFAULT`]: process.env.YARNSW_DEFAULT,
      },
    });

    if (process.env.JEST_LOG_SPAWNS) {
      console.log(`===== stdout:`);
      console.log(res.stdout);
      console.log(`===== stderr:`);
      console.log(res.stderr);
    }

    return res;
  },
  async runSwitchDriver(
    path,
    [command, ...args],
    {cwd, execArgv = [], projectFolder, registryUrl, env, stdin, ...config},
  ) {
    const rcEnv: Record<string, any> = {};
    for (const [key, value] of Object.entries(config))
      rcEnv[`YARN_${key.replace(/([A-Z])/g, `_$1`).toUpperCase()}`] = Array.isArray(value) ? value.join(`,`) : value;

    const nativePath = npath.fromPortablePath(path);
    const nativeHomePath = npath.dirname(nativePath);

    const cwdArgs = typeof projectFolder !== `undefined`
      ? [projectFolder]
      : [];

    const switchBinary = process.env.TEST_SWITCH_BINARY
      ?? require.resolve(`${__dirname}/../../../../../target/release/yarn`);

    const yarnBinBinary = getYarnBinaryPath();

    const switchBinaryArgs = switchBinary.match(/\.[cm]?js$/)
      ? [process.execPath, switchBinary]
      : [switchBinary];

    const res = await execFile(switchBinaryArgs[0]!, [...execArgv, ...switchBinaryArgs.slice(1), ...cwdArgs, command, ...args], {
      cwd: cwd || path,
      stdin,
      env: {
        ...baseEnv(nativePath, nativeHomePath, registryUrl, rcEnv, env),
        // Point Yarn Switch to the test registry for downloading Yarn releases
        [`YARNSW_NPM_REGISTRY_SERVER`]: registryUrl,
        // Use the local yarn-bin as the default when no packageManager field is present
        [`YARNSW_DEFAULT`]: `local:${yarnBinBinary}`,
      },
    });

    if (process.env.JEST_LOG_SPAWNS) {
      console.log(`===== stdout:`);
      console.log(res.stdout);
      console.log(`===== stderr:`);
      console.log(res.stderr);
    }

    return res;
  },
});

(global as any).makeTemporaryEnv = mte;

declare global {
  var makeTemporaryEnv: typeof mte;
}
