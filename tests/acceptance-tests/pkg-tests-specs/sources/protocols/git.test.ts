import {execUtils, semverUtils} from '@yarnpkg/core';
import {npath, ppath, xfs}      from '@yarnpkg/fslib';
import {tests}                  from 'pkg-tests-core';

const TESTED_URLS = {
  // We've picked util-deprecate because it doesn't have any dependency, and
  // thus doesn't crash when installing through our mock registry. We also
  // could have made our own repository (and maybe we will), but it was simpler
  // this way.
  //
  // Edit 2019 Dec 6 - we now have the ability to serve local repositories
  // through our test server (cf following tests); still, these tests are
  // useful since they test various different protocols such as ssh.

  [`git://github.com/yarnpkg/util-deprecate.git#v1.0.1`]: {version: `1.0.1`, runOnCI: false},
  [`git+ssh://git@github.com/yarnpkg/util-deprecate.git#v1.0.1`]: {version: `1.0.1`, runOnCI: false},
  [`https://github.com/yarnpkg/util-deprecate.git#semver:^1.0.0`]: {version: `1.0.2`, runOnCI: false},
  [`https://github.com/yarnpkg/util-deprecate.git#semver:>=1.0.0 <1.0.2`]: {version: `1.0.1`, runOnCI: false},
  [`https://github.com/yarnpkg/util-deprecate.git#v1.0.0`]: {version: `1.0.0`, runOnCI: true},
  [`https://github.com/yarnpkg/util-deprecate.git#master`]: {version: `1.0.2`, runOnCI: true},
  [`https://github.com/yarnpkg/util-deprecate.git#b3562c2798507869edb767da869cd7b85487726d`]: {version: `1.0.0`, runOnCI: true},
};

const makeCloneMetricsWrapper = ({realGitPath, metricsDir}: {realGitPath: string, metricsDir: string}) => `
#!/usr/bin/env node
const fs = require('fs');
const path = require('path');
const {spawn} = require('child_process');

const realGitPath = ${JSON.stringify(realGitPath)};
const metricsDir = ${JSON.stringify(metricsDir)};
const stateFile = path.join(metricsDir, 'state');
const lockDir = path.join(metricsDir, 'lock');

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

async function withLock(fn) {
  while (true) {
    try {
      await fs.promises.mkdir(lockDir);
      break;
    } catch (error) {
      if (error && error.code === 'EEXIST') {
        await sleep(10);
        continue;
      }

      throw error;
    }
  }

  try {
    return await fn();
  } finally {
    await fs.promises.rmdir(lockDir);
  }
}

async function readState() {
  const content = await fs.promises.readFile(stateFile, 'utf8');
  const [currentLine = '0', maxLine = '0'] = content.trim().split(/\\r\\n|\\r|\\n/);

  return {
    current: Number(currentLine),
    max: Number(maxLine),
  };
}

async function writeState(current, max) {
  await fs.promises.writeFile(stateFile, \`\${current}\\n\${max}\\n\`);
}

async function incrementCounter() {
  await withLock(async () => {
    const {current, max} = await readState();
    const nextCurrent = current + 1;
    const nextMax = Math.max(max, nextCurrent);

    await writeState(nextCurrent, nextMax);
  });
}

async function decrementCounter() {
  await withLock(async () => {
    const {current, max} = await readState();
    await writeState(current - 1, max);
  });
}

function runGit(args) {
  return new Promise((resolve, reject) => {
    const child = spawn(realGitPath, args, {stdio: 'inherit'});

    child.on('error', reject);
    child.on('exit', code => {
      resolve(typeof code === 'number' ? code : 1);
    });
  });
}

async function main() {
  const args = process.argv.slice(2);

  if (args[0] === 'clone') {
    await incrementCounter();
    await sleep(200);

    const exitCode = await (async () => {
      try {
        return await runGit(args);
      } finally {
        await decrementCounter();
      }
    })();

    process.exit(exitCode);
  }

  process.exit(await runGit(args));
}

main().catch(error => {
  console.error(error);
  process.exit(1);
});
`.trimStart();

describe(`Protocols`, () => {
  describe(`git:`, () => {
    for (const [url, {version, runOnCI}] of Object.entries(TESTED_URLS)) {
      const testFn = !process.env.GITHUB_ACTIONS || runOnCI
        ? test
        : test.skip;

      testFn(
        `it should resolve a git dependency (${url})`,
        makeTemporaryEnv(
          {
            dependencies: {[`util-deprecate`]: url},
          },
          async ({path, run, source}) => {
            await run(`install`);

            await expect(source(`require('util-deprecate/package.json')`)).resolves.toMatchObject({
              name: `util-deprecate`,
              version,
            });
          },
        ),
      );
    }

    test(
      `it should install dependencies and run prepack if needed`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`has-prepack`]: tests.startPackageServer().then(url => `${url}/repositories/has-prepack.git`),
          },
        },
        async ({path, run, source}) => {
          await run(`install`);

          await expect(source(`require('has-prepack')`)).resolves.toEqual(42);
        },
      ),
    );

    test(
      `it shouldn't install dependencies for packages without prepack`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`no-prepack`]: tests.startPackageServer().then(url => `${url}/repositories/no-prepack.git`),
          },
        },
        async ({path, run, source}) => {
          await run(`install`);

          await expect(source(`require('no-prepack')`)).resolves.toEqual(42);
        },
      ),
    );

    test(
      `it should support installing packages from projects in subfolders`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`pkg-a`]: tests.startPackageServer().then(url => `${url}/repositories/deep-projects.git#cwd=projects/pkg-a`),
            [`pkg-b`]: tests.startPackageServer().then(url => `${url}/repositories/deep-projects.git#cwd=projects/pkg-b`),
          },
        },
        async ({path, run, source}) => {
          await run(`install`);

          await expect(source(`require('pkg-a/package.json')`)).resolves.toMatchObject({
            name: `pkg-a`,
            version: `1.0.0`,
          });

          await expect(source(`require('pkg-b/package.json')`)).resolves.toMatchObject({
            name: `pkg-b`,
            version: `1.0.0`,
          });
        },
      ),
    );

    test(
      `it should support installing workspace packages from projects in subfolders`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`lib-a`]: tests.startPackageServer().then(url => `${url}/repositories/deep-projects.git#cwd=projects/pkg-a&workspace=lib`),
            [`lib-b`]: tests.startPackageServer().then(url => `${url}/repositories/deep-projects.git#cwd=projects/pkg-b&workspace=lib`),
          },
        },
        async ({path, run, source}) => {
          await run(`install`);

          await expect(source(`require('lib-a/package.json')`)).resolves.toMatchObject({
            name: `lib`,
            version: `1.0.0`,
          });

          await expect(source(`require('lib-b/package.json')`)).resolves.toMatchObject({
            name: `lib`,
            version: `1.0.0`,
          });

          await expect(source(`require('lib-a')`)).resolves.toEqual(`pkg-a`);
          await expect(source(`require('lib-b')`)).resolves.toEqual(`pkg-b`);
        },
      ),
    );

    test(
      `it should support installing specific workspaces`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`pkg-a`]: tests.startPackageServer().then(url => `${url}/repositories/workspaces.git#workspace=pkg-a`),
            [`pkg-b`]: tests.startPackageServer().then(url => `${url}/repositories/workspaces.git#workspace=pkg-b`),
          },
        },
        async ({path, run, source}) => {
          await run(`install`);

          await expect(source(`require('pkg-a/package.json')`)).resolves.toMatchObject({
            name: `pkg-a`,
            version: `1.0.0`,
          });

          await expect(source(`require('pkg-b/package.json')`)).resolves.toMatchObject({
            name: `pkg-b`,
            version: `1.0.0`,
          });
        },
      ),
    );

    tests.testIf(
      () => process.platform !== `win32`,
      `it should respect cloneConcurrency when cloning git repositories`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`pkg-a`]: tests.startPackageServer().then(url => `${url}/repositories/deep-projects.git#cwd=projects/pkg-a`),
            [`pkg-b`]: tests.startPackageServer().then(url => `${url}/repositories/deep-projects.git#cwd=projects/pkg-b`),
            [`lib-a`]: tests.startPackageServer().then(url => `${url}/repositories/deep-projects.git#cwd=projects/pkg-a&workspace=lib`),
            [`lib-b`]: tests.startPackageServer().then(url => `${url}/repositories/deep-projects.git#cwd=projects/pkg-b&workspace=lib`),
          },
        },
        {
          cloneConcurrency: 1,
        },
        async ({path, run, source}) => {
          const {stdout: gitPathStdout} = await execUtils.execvp(`which`, [`git`], {cwd: path});
          const realGitPath = gitPathStdout.trim();

          const binDir = ppath.join(path, `bin`);
          const metricsDir = ppath.join(path, `.clone-metrics`);
          const stateFile = ppath.join(metricsDir, `state`);
          const wrapperPath = ppath.join(binDir, `git`);

          await xfs.mkdirPromise(binDir, {recursive: true});
          await xfs.mkdirPromise(metricsDir, {recursive: true});
          await xfs.writeFilePromise(stateFile, `0\n0\n`);
          await xfs.writeFilePromise(wrapperPath, makeCloneMetricsWrapper({
            realGitPath,
            metricsDir: npath.fromPortablePath(metricsDir),
          }));

          await xfs.chmodPromise(wrapperPath, 0o755);

          await run(`install`);

          const [currentLine, maxLine] = (await xfs.readFilePromise(stateFile, `utf8`)).trim().split(/\r\n|\r|\n/);

          expect(Number(currentLine)).toBe(0);
          expect(Number(maxLine)).toBeGreaterThan(0);
          expect(Number(maxLine)).toBeLessThanOrEqual(1);
        },
      ),
    );

    test(
      `it should use Yarn Classic to setup classic repositories`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`yarn-1-project`]: tests.startPackageServer().then(url => `${url}/repositories/yarn-1-project.git`),
          },
        },
        async ({path, run, source}) => {
          await expect(run(`install`, {
            env: {
              // if this is set then yarn 1 will be executed as if `--production` was passed during the install
              // but `yarn-1-project` requires dev dependencies to be present so this is a good way to
              // verify that yarn isn't throw off by this when handling the clone, install, and pack process
              // for git dependencies (see: https://classic.yarnpkg.com/lang/en/docs/cli/install/#toc-yarn-install-production-true-false)
              NODE_ENV: `production`,
            },
          })).resolves.toBeTruthy();

          await expect(source(`require('yarn-1-project')`)).resolves.toMatch(/\byarn\/1\.[0-9]+/);
        },
      ),
    );

    test(
      `it should use npm to setup npm repositories`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`npm-project`]: tests.startPackageServer().then(url => `${url}/repositories/npm-project.git`),
          },
        },
        async ({path, run, source}) => {
          await run(`install`);

          await expect(source(`require('npm-project')`)).resolves.toMatch(/\bnpm\/[0-9]+/);
        },
      ),
    );

    test(
      `it should guarantee that all dependencies will be installed when using npm to setup npm repositories`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`npm-has-prepack`]: tests.startPackageServer().then(url => `${url}/repositories/npm-has-prepack.git`),
          },
        },
        async ({path, run, source}) => {
          await expect(run(`install`, {
            env: {
              // if this is set then npm will be executed as if `--omit=dev` was passed during the install
              // but `has-prepack-npm` requires dev dependencies to be present so this is a good way to
              // verify that yarn isn't throw off by this when handling the clone, install, and pack process
              // for git dependencies (see: https://docs.npmjs.com/cli/v8/using-npm/config#omit)
              NODE_ENV: `production`,

              // same for NPM_CONFIG_PRODUCTION which acts just like the `--production` flat during install step
              // (see: https://docs.npmjs.com/cli/v8/using-npm/config#environment-variables, https://docs.npmjs.com/cli/v8/using-npm/config#production)
              NPM_CONFIG_PRODUCTION: `true`,
              npm_config_production: `true`,

              // also force npm to use the package server as the registry so that the `has-bin-entry` dependency can be resolved
              NPM_CONFIG_REGISTRY: await tests.startPackageServer(),
            },
          })).resolves.toBeTruthy();
          await expect(source(`require('npm-has-prepack')`)).resolves.toEqual(42);
        },
      ),
    );

    test(
      `it should support installing specific workspaces from npm repositories`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`pkg-a`]: tests.startPackageServer().then(url => `${url}/repositories/npm-workspaces.git#workspace=pkg-a`),
            [`pkg-b`]: tests.startPackageServer().then(url => `${url}/repositories/npm-workspaces.git#workspace=pkg-b`),
          },
        },
        async ({path, run, source}) => {
          const {code, stdout, stderr} = await execUtils.execvp(`npm`, [`--version`], {cwd: path});
          if (code !== 0)
            throw new Error(`Couldn't get npm version: ${stderr}`);

          const npmVersion = stdout.trim();
          const doesNpmSupportWorkspaces = semverUtils.satisfiesWithPrereleases(npmVersion, `>=7.x`);

          if (doesNpmSupportWorkspaces) {
            await run(`install`);

            await expect(source(`require('pkg-a/package.json')`)).resolves.toMatchObject({
              name: `pkg-a`,
              version: `1.0.0`,
            });

            await expect(source(`require('pkg-b/package.json')`)).resolves.toMatchObject({
              name: `pkg-b`,
              version: `1.0.0`,
            });
          } else {
            await expect(run(`install`)).rejects.toThrow(`Workspaces aren't supported by npm@${npmVersion}`);
          }
        },
      ),
    );

    test(
      `it should not use Corepack to fetch Yarn Classic`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`yarn-1-project`]: tests.startPackageServer().then(url => `${url}/repositories/yarn-1-project.git`),
          },
        },
        async ({path, run, source}) => {
          // This checks that preparing Yarn Classic repositories doesn't use Corepack.
          await expect(run(`install`, {
            env: {
              COREPACK_ROOT: npath.join(npath.fromPortablePath(path), `404`),
              YARN_ENABLE_INLINE_BUILDS: `true`,
            },
          })).resolves.toBeDefined();
        },
      ),
    );

    test(
      `it should not use Corepack to install repositories that are installed via Yarn 2+`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`no-lockfile-project`]: tests.startPackageServer().then(url => `${url}/repositories/no-lockfile-project.git`),
          },
        },
        async ({path, run, source}) => {
          await expect(run(`install`, {
            env: {
              COREPACK_ROOT: npath.join(npath.fromPortablePath(path), `404`),
              YARN_ENABLE_INLINE_BUILDS: `true`,
            },
          })).resolves.toBeDefined();
        },
      ),
    );

    test(
      `it should not add a 'packageManager' field to a Yarn classic project`,
      makeTemporaryEnv(
        {
          dependencies: {
            [`yarn-1-project`]: tests.startPackageServer().then(url => `${url}/repositories/yarn-1-project.git`),
          },
        },
        async ({path, run, source}) => {
          await expect(run(`install`)).resolves.toBeTruthy();

          await expect(source(`require('yarn-1-project/package.json').packageManager`)).resolves.toBeUndefined();
        },
      ),
    );
  });
});
