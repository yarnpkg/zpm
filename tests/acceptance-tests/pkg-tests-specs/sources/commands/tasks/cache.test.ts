import {npath, ppath, xfs, PortablePath, Filename} from '@yarnpkg/fslib';

import {RunFunction}                         from '../../../../pkg-tests-core/sources/utils/tests';

function cleanupDaemon(cb: RunFunction): RunFunction {
  return async args => {
    try {
      await cb(args);
    } finally {
      await args.runSwitch(`switch`, `daemon`, `--kill-all`);
    }
  };
}

// The counter files live outside of the outputs (and of the inputs), so we
// can tell how many times each script actually ran. They're written by
// absolute path since the scripts run from their own workspace.
let countersPath: PortablePath;

async function readCounter(path: PortablePath, name: string) {
  const counterPath = ppath.join(countersPath, name as PortablePath);
  if (!xfs.existsSync(counterPath))
    return 0;

  const content = await xfs.readFilePromise(counterPath, `utf8`);
  return content.split(`\n`).filter(line => line.length > 0).length;
}

function countRun(name: string) {
  return `echo run >> "${npath.fromPortablePath(ppath.join(countersPath, name as PortablePath))}"`;
}

async function writeTaskfile(path: PortablePath, lines: Array<string>) {
  await xfs.writeFilePromise(ppath.join(path, `taskfile`), lines.join(`\n`));
}

async function setupProject(path: PortablePath) {
  countersPath = await xfs.mktempPromise();
}

describe(`Commands`, () => {
  describe(`tasks run (cache)`, () => {
    test(
      `it should combine @default inputs with files outside of the workspace`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/app`]: {name: `app`},
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);

        const appPath = ppath.join(path, `packages/app` as PortablePath);
        await xfs.mkdirPromise(ppath.join(path, `shared`));
        await xfs.writeFilePromise(ppath.join(path, `shared/spec.json`), `{}`);
        await xfs.writeFilePromise(ppath.join(appPath, `index.txt`), `v1`);
        await xfs.writeFilePromise(ppath.join(appPath, `notes.md`), `ignored`);

        await xfs.writeFilePromise(ppath.join(appPath, `taskfile`), [
          `@cache`,
          `@inputs(@default !notes.md ../../shared/**)`,
          `build:`,
          `  ${countRun(`build`)}`,
        ].join(`\n`));

        await run(`install`);

        const build = () => runSwitch(`tasks`, `run`, `--standalone`, `build`, {cwd: appPath});

        await build();
        await build();
        expect(await readCounter(path, `build`)).toEqual(1);

        // A workspace file (from @default) invalidates
        await xfs.writeFilePromise(ppath.join(appPath, `index.txt`), `v2`);
        await build();
        expect(await readCounter(path, `build`)).toEqual(2);

        // A file outside of the workspace invalidates
        await xfs.writeFilePromise(ppath.join(path, `shared/spec.json`), `{"a":1}`);
        await build();
        expect(await readCounter(path, `build`)).toEqual(3);

        // Exclusions apply to the @default set too
        await xfs.writeFilePromise(ppath.join(appPath, `notes.md`), `changed`);
        await build();
        expect(await readCounter(path, `build`)).toEqual(3);
      })),
    );

    test(
      `it should walk input roots that are symlinks to folders`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);

        const shared = await xfs.mktempPromise();
        await xfs.writeFilePromise(ppath.join(shared, `index.txt`), `v1`);
        await xfs.symlinkPromise(shared, ppath.join(path, `src`));

        await writeTaskfile(path, [
          `@cache`,
          `@inputs(src/**)`,
          `build:`,
          `  ${countRun(`build`)}`,
        ]);

        await run(`install`);

        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(await readCounter(path, `build`)).toEqual(1);

        await xfs.writeFilePromise(ppath.join(shared, `index.txt`), `v2`);
        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(await readCounter(path, `build`)).toEqual(2);
      })),
    );

    test(
      `it should run the task when its cached outputs can't be restored`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);
        await xfs.writeFilePromise(ppath.join(path, `input.txt`), `v1`);

        await writeTaskfile(path, [
          `@cache`,
          `@inputs(input.txt)`,
          `@outputs(dist/**)`,
          `build:`,
          `  ${countRun(`build`)}`,
          `  mkdir -p dist/out && echo built > dist/out/file.txt`,
        ]);

        await run(`install`);

        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(await readCounter(path, `build`)).toEqual(1);

        // A read-only folder in the way makes the restore fail midway
        await xfs.removePromise(ppath.join(path, `dist`));
        await xfs.mkdirPromise(ppath.join(path, `dist`));
        await xfs.chmodPromise(ppath.join(path, `dist`), 0o555);

        try {
          await runSwitch(`tasks`, `run`, `--standalone`, `build`).catch(() => {});
        } finally {
          await xfs.chmodPromise(ppath.join(path, `dist`), 0o755);
        }

        // The failed restore falls back to running the script
        expect(await readCounter(path, `build`)).toEqual(2);
      })),
    );

    test(
      `tasks hash should fingerprint cached tasks without a script`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);

        await writeTaskfile(path, [
          `@cache`,
          `lib:`,
          `  echo lib`,
          ``,
          `@cache`,
          `build: lib`,
        ]);

        await run(`install`);

        const {stdout} = await runSwitch(`tasks`, `hash`, `build`);
        expect(stdout).toContain(`test-package:build`);
        expect(stdout).toContain(`Fingerprint:`);
      })),
    );

    test(
      `it should skip the script and replay the output on the second run`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);
        await xfs.mkdirPromise(ppath.join(path, `src`));
        await xfs.writeFilePromise(ppath.join(path, `src/index.txt`), `hello`);

        await writeTaskfile(path, [
          `@cache`,
          `@inputs(src/**)`,
          `@outputs(dist/**)`,
          `build:`,
          `  ${countRun(`build`)}`,
          `  mkdir -p dist && cp src/index.txt dist/index.txt`,
          `  echo "building"`,
          `  echo "warning" >&2`,
        ]);

        await run(`install`);

        const first = await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(first.stdout).toEqual(`building\nwarning\n`);
        expect(first.stderr).not.toContain(`Cache hit`);
        expect(await readCounter(path, `build`)).toEqual(1);

        const second = await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(second.stdout).toEqual(`building\nwarning\n`);
        expect(second.stderr).toContain(`[test-package:build]: Cache hit, replaying output`);
        expect(await readCounter(path, `build`)).toEqual(1);

        const verbose = await runSwitch(`tasks`, `run`, `--standalone`, `-v`, `build`);
        expect(verbose.stdout).toEqual([
          `[test-package:build]: Cache hit, replaying output`,
          `[test-package:build]: building`,
          `[test-package:build]: warning`,
          ``,
        ].join(`\n`));
        expect(await readCounter(path, `build`)).toEqual(1);
      })),
    );

    test(
      `it should report cache hits through \`yarn run\``,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);

        await writeTaskfile(path, [
          `@cache`,
          `build:`,
          `  ${countRun(`build`)}`,
          `  echo "building"`,
        ]);

        await run(`install`);

        await expect(runSwitch(`run`, `build`)).resolves.toMatchObject({stdout: `building\n`});
        await expect(runSwitch(`build`)).resolves.toMatchObject({stdout: `building\n`});
        expect(await readCounter(path, `build`)).toEqual(1);
      })),
    );

    test(
      `it should miss when an input file changes`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);
        await xfs.mkdirPromise(ppath.join(path, `src`));
        await xfs.writeFilePromise(ppath.join(path, `src/index.txt`), `v1`);
        await xfs.writeFilePromise(ppath.join(path, `README.md`), `readme`);

        await writeTaskfile(path, [
          `@cache`,
          `@inputs(src/**)`,
          `build:`,
          `  ${countRun(`build`)}`,
        ]);

        await run(`install`);

        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(await readCounter(path, `build`)).toEqual(1);

        // Not an input
        await xfs.writeFilePromise(ppath.join(path, `README.md`), `changed readme`);
        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(await readCounter(path, `build`)).toEqual(1);

        // Same size, so only the content hash can tell
        await xfs.writeFilePromise(ppath.join(path, `src/index.txt`), `v2`);
        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(await readCounter(path, `build`)).toEqual(2);

        await xfs.writeFilePromise(ppath.join(path, `src/new.txt`), `new file`);
        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(await readCounter(path, `build`)).toEqual(3);

        await xfs.removePromise(ppath.join(path, `src/new.txt`));
        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        // Back to a state already seen
        expect(await readCounter(path, `build`)).toEqual(3);
      })),
    );

    test(
      `it should use all the non-ignored workspace files as inputs by default`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);
        await xfs.mkdirPromise(ppath.join(path, `.git`));
        await xfs.writeFilePromise(ppath.join(path, `.gitignore`), `ignored.txt\n`);
        await xfs.writeFilePromise(ppath.join(path, `file.txt`), `v1`);

        await writeTaskfile(path, [
          `@cache`,
          `@outputs(dist/**)`,
          `build:`,
          `  ${countRun(`build`)}`,
          `  mkdir -p dist && echo built > dist/out.txt`,
        ]);

        await run(`install`);

        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(await readCounter(path, `build`)).toEqual(1);

        // Ignored files and outputs aren't inputs
        await xfs.writeFilePromise(ppath.join(path, `ignored.txt`), `ignored`);
        await xfs.writeFilePromise(ppath.join(path, `dist/extra.txt`), `extra`);
        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(await readCounter(path, `build`)).toEqual(1);

        await xfs.writeFilePromise(ppath.join(path, `file.txt`), `v2`);
        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(await readCounter(path, `build`)).toEqual(2);
      })),
    );

    test(
      `it should miss when the script changes`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);

        await writeTaskfile(path, [
          `@cache`,
          `build:`,
          `  ${countRun(`build`)}`,
          `  echo "v1"`,
        ]);

        await run(`install`);

        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(await readCounter(path, `build`)).toEqual(1);

        await writeTaskfile(path, [
          `@cache`,
          `build:`,
          `  ${countRun(`build`)}`,
          `  echo "v2"`,
        ]);

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(stdout).toEqual(`v2\n`);
        expect(await readCounter(path, `build`)).toEqual(2);
      })),
    );

    test(
      `it should cascade dependency changes to dependent tasks`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/lib`]: {
          name: `lib`,
        },
        [`packages/app`]: {
          name: `app`,
          dependencies: {
            [`lib`]: `workspace:*`,
          },
        },
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);

        const libPath = ppath.join(path, `packages/lib` as PortablePath);
        const appPath = ppath.join(path, `packages/app` as PortablePath);

        await xfs.writeFilePromise(ppath.join(libPath, `lib.txt`), `lib v1`);
        await xfs.writeFilePromise(ppath.join(appPath, `app.txt`), `app v1`);

        await writeTaskfile(libPath, [
          `@cache`,
          `@inputs(lib.txt)`,
          `@outputs(dist/**)`,
          `build:`,
          `  ${countRun(`lib`)}`,
          `  mkdir -p dist && cp lib.txt dist/lib.txt`,
        ]);

        await writeTaskfile(appPath, [
          `@cache`,
          `@inputs(app.txt)`,
          `@outputs(dist/**)`,
          `build: lib:build`,
          `  ${countRun(`app`)}`,
          `  mkdir -p dist && cat app.txt ../lib/dist/lib.txt > dist/app.txt`,
        ]);

        await run(`install`);

        await runSwitch(`tasks`, `run`, `--standalone`, `build`, {cwd: appPath});
        expect(await readCounter(path, `lib`)).toEqual(1);
        expect(await readCounter(path, `app`)).toEqual(1);

        await runSwitch(`tasks`, `run`, `--standalone`, `build`, {cwd: appPath});
        expect(await readCounter(path, `lib`)).toEqual(1);
        expect(await readCounter(path, `app`)).toEqual(1);

        // Changing the dependency invalidates both tasks
        await xfs.writeFilePromise(ppath.join(libPath, `lib.txt`), `lib v2`);
        await runSwitch(`tasks`, `run`, `--standalone`, `build`, {cwd: appPath});
        expect(await readCounter(path, `lib`)).toEqual(2);
        expect(await readCounter(path, `app`)).toEqual(2);
        await expect(xfs.readFilePromise(ppath.join(appPath, `dist/app.txt`), `utf8`)).resolves.toEqual(`app v1lib v2`);

        // Changing the dependent only invalidates the dependent
        await xfs.writeFilePromise(ppath.join(appPath, `app.txt`), `app v2`);
        await runSwitch(`tasks`, `run`, `--standalone`, `build`, {cwd: appPath});
        expect(await readCounter(path, `lib`)).toEqual(2);
        expect(await readCounter(path, `app`)).toEqual(3);
      })),
    );

    test(
      `it should cascade changes through dependencies that aren't cached`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);
        await xfs.writeFilePromise(ppath.join(path, `source.txt`), `v1`);

        // Generated files are typically ignored; otherwise they'd be part of
        // the default inputs of the task generating them
        await xfs.mkdirPromise(ppath.join(path, `.git`));
        await xfs.writeFilePromise(ppath.join(path, `.gitignore`), `generated.txt\n`);

        await writeTaskfile(path, [
          `generate:`,
          `  ${countRun(`generate`)}`,
          `  cp source.txt generated.txt`,
          ``,
          `@cache`,
          `@inputs(generated.txt)`,
          `build: generate`,
          `  ${countRun(`build`)}`,
        ]);

        await run(`install`);

        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        await runSwitch(`tasks`, `run`, `--standalone`, `build`);

        // Uncached tasks always run
        expect(await readCounter(path, `generate`)).toEqual(2);
        expect(await readCounter(path, `build`)).toEqual(1);

        await writeTaskfile(path, [
          `generate:`,
          `  ${countRun(`generate`)}`,
          `  cp source.txt generated.txt`,
          `  echo changed >> generated.txt`,
          ``,
          `@cache`,
          `@inputs(generated.txt)`,
          `build: generate`,
          `  ${countRun(`build`)}`,
        ]);

        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(await readCounter(path, `build`)).toEqual(2);
      })),
    );

    test(
      `it should miss when a declared environment variable changes`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);

        await writeTaskfile(path, [
          `@cache`,
          `@env(MY_VAR MY_PREFIX_*)`,
          `build:`,
          `  ${countRun(`build`)}`,
          `  echo "value=$MY_VAR"`,
        ]);

        await run(`install`);

        await runSwitch(`tasks`, `run`, `--standalone`, `build`, {env: {MY_VAR: `a`}});
        await runSwitch(`tasks`, `run`, `--standalone`, `build`, {env: {MY_VAR: `a`}});
        expect(await readCounter(path, `build`)).toEqual(1);

        // Undeclared variables don't matter
        await runSwitch(`tasks`, `run`, `--standalone`, `build`, {env: {MY_VAR: `a`, OTHER_VAR: `x`}});
        expect(await readCounter(path, `build`)).toEqual(1);

        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `build`, {env: {MY_VAR: `b`}});
        expect(stdout).toEqual(`value=b\n`);
        expect(await readCounter(path, `build`)).toEqual(2);

        // Unset differs from set
        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(await readCounter(path, `build`)).toEqual(3);

        // Prefix patterns
        await runSwitch(`tasks`, `run`, `--standalone`, `build`, {env: {MY_PREFIX_FOO: `1`}});
        expect(await readCounter(path, `build`)).toEqual(4);
        await runSwitch(`tasks`, `run`, `--standalone`, `build`, {env: {MY_PREFIX_FOO: `1`}});
        expect(await readCounter(path, `build`)).toEqual(4);
      })),
    );

    test(
      `it should restore the outputs (with their modes) after they got deleted`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);

        await writeTaskfile(path, [
          `@cache`,
          `@outputs(dist/** lib)`,
          `build:`,
          `  ${countRun(`build`)}`,
          `  mkdir -p dist/nested lib`,
          `  echo "main" > dist/main.js`,
          `  echo "nested" > dist/nested/file.js`,
          `  printf '#!/bin/sh\\necho hi\\n' > dist/bin.sh && chmod 755 dist/bin.sh`,
          `  echo "lib" > lib/index.js`,
          `  ln -s main.js dist/link.js`,
        ]);

        await run(`install`);

        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(await readCounter(path, `build`)).toEqual(1);

        await xfs.removePromise(ppath.join(path, `dist`));
        await xfs.removePromise(ppath.join(path, `lib`));

        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(await readCounter(path, `build`)).toEqual(1);

        await expect(xfs.readFilePromise(ppath.join(path, `dist/main.js`), `utf8`)).resolves.toEqual(`main\n`);
        await expect(xfs.readFilePromise(ppath.join(path, `dist/nested/file.js`), `utf8`)).resolves.toEqual(`nested\n`);
        await expect(xfs.readFilePromise(ppath.join(path, `lib/index.js`), `utf8`)).resolves.toEqual(`lib\n`);
        await expect(xfs.readlinkPromise(ppath.join(path, `dist/link.js`))).resolves.toEqual(`main.js`);

        const binStat = await xfs.statPromise(ppath.join(path, `dist/bin.sh`));
        expect(binStat.mode & 0o777).toEqual(0o755);

        const mainStat = await xfs.statPromise(ppath.join(path, `dist/main.js`));
        expect(mainStat.mode & 0o111).toEqual(0);
      })),
    );

    test(
      `it should clear stale outputs before restoring`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);

        await writeTaskfile(path, [
          `@cache`,
          `@inputs()`,
          `@outputs(dist/**)`,
          `build:`,
          `  ${countRun(`build`)}`,
          `  mkdir -p dist && echo "fresh" > dist/main.js`,
        ]);

        await run(`install`);

        await runSwitch(`tasks`, `run`, `--standalone`, `build`);

        await xfs.writeFilePromise(ppath.join(path, `dist/main.js`), `tampered`);
        await xfs.mkdirPromise(ppath.join(path, `dist/stale`));
        await xfs.writeFilePromise(ppath.join(path, `dist/stale/old.js`), `stale`);
        await xfs.writeFilePromise(ppath.join(path, `untouched.txt`), `untouched`);

        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(await readCounter(path, `build`)).toEqual(1);

        await expect(xfs.readFilePromise(ppath.join(path, `dist/main.js`), `utf8`)).resolves.toEqual(`fresh\n`);
        expect(xfs.existsSync(ppath.join(path, `dist/stale`))).toEqual(false);
        expect(xfs.existsSync(ppath.join(path, `untouched.txt`))).toEqual(true);
      })),
    );

    test(
      `it should cache tasks whose outputs match nothing`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);

        await writeTaskfile(path, [
          `@cache`,
          `@outputs(dist/**)`,
          `lint:`,
          `  ${countRun(`lint`)}`,
          `  echo "all good"`,
        ]);

        await run(`install`);

        await runSwitch(`tasks`, `run`, `--standalone`, `lint`);
        const {stdout} = await runSwitch(`tasks`, `run`, `--standalone`, `lint`);

        expect(stdout).toEqual(`all good\n`);
        expect(await readCounter(path, `lint`)).toEqual(1);
        expect(xfs.existsSync(ppath.join(path, `dist`))).toEqual(false);
      })),
    );

    test(
      `it should not cache failed tasks`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);

        await writeTaskfile(path, [
          `@cache`,
          `build:`,
          `  ${countRun(`build`)}`,
          `  echo "failing"`,
          `  exit 1`,
        ]);

        await run(`install`);

        await expect(runSwitch(`tasks`, `run`, `--standalone`, `build`)).rejects.toMatchObject({code: 1});
        await expect(runSwitch(`tasks`, `run`, `--standalone`, `build`)).rejects.toMatchObject({code: 1});

        expect(await readCounter(path, `build`)).toEqual(2);
      })),
    );

    test(
      `it should always run tasks without @cache`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);

        await writeTaskfile(path, [
          `build:`,
          `  ${countRun(`build`)}`,
          `  echo "building"`,
        ]);

        await run(`install`);

        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        const {stdout, stderr} = await runSwitch(`tasks`, `run`, `--standalone`, `build`);

        expect(stdout).toEqual(`building\n`);
        expect(stderr).not.toContain(`Cache hit`);
        expect(await readCounter(path, `build`)).toEqual(2);
        expect(xfs.existsSync(ppath.join(path, `.yarn/ignore/task-cache` as PortablePath))).toEqual(false);
      })),
    );

    test(
      `it should bypass the cache with --no-cache`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);

        await writeTaskfile(path, [
          `@cache`,
          `build:`,
          `  ${countRun(`build`)}`,
          `  echo "building"`,
        ]);

        await run(`install`);

        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        await runSwitch(`tasks`, `run`, `--standalone`, `--no-cache`, `build`);
        expect(await readCounter(path, `build`)).toEqual(2);

        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(await readCounter(path, `build`)).toEqual(2);
      })),
    );

    test(
      `it should not use the cache when enableTaskCache is false`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);

        await writeTaskfile(path, [
          `@cache`,
          `build:`,
          `  ${countRun(`build`)}`,
        ]);

        await run(`install`);

        await runSwitch(`tasks`, `run`, `--standalone`, `build`, {env: {YARN_ENABLE_TASK_CACHE: `false`}});
        await runSwitch(`tasks`, `run`, `--standalone`, `build`, {env: {YARN_ENABLE_TASK_CACHE: `false`}});
        expect(await readCounter(path, `build`)).toEqual(2);
      })),
    );

    test(
      `it should store the entries in taskCacheFolder`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);

        await writeTaskfile(path, [
          `@cache`,
          `build:`,
          `  ${countRun(`build`)}`,
        ]);

        await run(`install`);

        await runSwitch(`tasks`, `run`, `--standalone`, `build`, {env: {YARN_TASK_CACHE_FOLDER: `custom-cache`}});
        expect(xfs.existsSync(ppath.join(path, `custom-cache` as PortablePath))).toEqual(true);
        expect(xfs.existsSync(ppath.join(path, `.yarn/ignore/task-cache` as PortablePath))).toEqual(false);

        await runSwitch(`tasks`, `run`, `--standalone`, `build`, {env: {YARN_TASK_CACHE_FOLDER: `custom-cache`}});
        expect(await readCounter(path, `build`)).toEqual(1);
      })),
    );

    test(
      `it should miss when a global input changes`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);
        await xfs.writeFilePromise(ppath.join(path, `.nvmrc`), `22\n`);
        await xfs.writeFilePromise(ppath.join(path, `.yarnrc.yml` as Filename), `taskCacheGlobalInputs: [".nvmrc"]\n`);

        await writeTaskfile(path, [
          `@cache`,
          `@inputs()`,
          `build:`,
          `  ${countRun(`build`)}`,
        ]);

        await run(`install`);

        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(await readCounter(path, `build`)).toEqual(1);

        await xfs.writeFilePromise(ppath.join(path, `.nvmrc`), `24\n`);
        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        expect(await readCounter(path, `build`)).toEqual(2);
      })),
    );

    test(
      `it should miss when the installed dependencies change`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/app`]: {
          name: `app`,
          dependencies: {
            [`no-deps`]: `1.0.0`,
          },
        },
        [`packages/other`]: {
          name: `other`,
        },
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);

        const appPath = ppath.join(path, `packages/app` as PortablePath);
        const otherPath = ppath.join(path, `packages/other` as PortablePath);

        await writeTaskfile(appPath, [
          `@cache`,
          `@inputs()`,
          `build:`,
          `  ${countRun(`app`)}`,
        ]);

        await writeTaskfile(otherPath, [
          `@cache`,
          `@inputs()`,
          `build:`,
          `  ${countRun(`other`)}`,
        ]);

        await run(`install`);

        await runSwitch(`tasks`, `run`, `--standalone`, `build`, {cwd: appPath});
        await runSwitch(`tasks`, `run`, `--standalone`, `build`, {cwd: otherPath});
        expect(await readCounter(path, `app`)).toEqual(1);
        expect(await readCounter(path, `other`)).toEqual(1);

        // Same range in the manifest, different resolution in the lockfile
        await run(`set`, `resolution`, `no-deps@npm:1.0.0`, `npm:2.0.0`);

        await runSwitch(`tasks`, `run`, `--standalone`, `build`, {cwd: appPath});
        await runSwitch(`tasks`, `run`, `--standalone`, `build`, {cwd: otherPath});
        expect(await readCounter(path, `app`)).toEqual(2);
        // Workspaces whose dependency tree didn't change aren't affected
        expect(await readCounter(path, `other`)).toEqual(1);
      })),
    );

    test(
      `it should reject @cache on long-lived tasks`,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);
        await writeTaskfile(path, [
          `@cache`,
          `@long-lived`,
          `dev:`,
          `  echo "serving"`,
        ]);

        await run(`install`);

        await expect(runSwitch(`tasks`, `run`, `--standalone`, `dev`)).rejects.toMatchObject({
          stdout: expect.stringContaining(`long-lived tasks cannot be cached`),
        });
      })),
    );

    test(
      `it should remove all entries with \`tasks cache clean\``,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);

        await writeTaskfile(path, [
          `@cache`,
          `build:`,
          `  ${countRun(`build`)}`,
        ]);

        await run(`install`);

        await runSwitch(`tasks`, `run`, `--standalone`, `build`);
        await runSwitch(`tasks`, `cache`, `clean`);
        await runSwitch(`tasks`, `run`, `--standalone`, `build`);

        expect(await readCounter(path, `build`)).toEqual(2);
      })),
    );

    test(
      `it should print the fingerprint the runner uses with \`tasks hash\``,
      makeTemporaryEnv({
        name: `test-package`,
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);
        await xfs.mkdirPromise(ppath.join(path, `src`));
        await xfs.writeFilePromise(ppath.join(path, `src/index.txt`), `v1`);

        await writeTaskfile(path, [
          `prepare:`,
          `  ${countRun(`prepare`)}`,
          ``,
          `@cache`,
          `@inputs(src/**)`,
          `@env(MY_VAR)`,
          `build: prepare`,
          `  ${countRun(`build`)}`,
        ]);

        await run(`install`);

        const before = JSON.parse((await runSwitch(`tasks`, `hash`, `--json`, `build`, {env: {MY_VAR: `secret-value`}})).stdout.trim().split(`\n`).pop()!);
        expect(before).toMatchObject({
          task: `test-package:build`,
          isCached: true,
          hasEntry: false,
          inputs: {
            [`package.json`]: expect.any(String),
            [`src/index.txt`]: expect.any(String),
          },
          dependencies: {
            [`test-package:prepare`]: expect.any(String),
          },
        });

        // Values are hashed, never printed
        expect(JSON.stringify(before)).not.toContain(`secret-value`);
        expect(before.env.MY_VAR).toEqual(expect.any(String));

        await runSwitch(`tasks`, `run`, `--standalone`, `build`, {env: {MY_VAR: `secret-value`}});

        const after = JSON.parse((await runSwitch(`tasks`, `hash`, `--json`, `build`, {env: {MY_VAR: `secret-value`}})).stdout.trim().split(`\n`).pop()!);
        expect(after).toMatchObject({fingerprint: before.fingerprint, hasEntry: true});

        await xfs.writeFilePromise(ppath.join(path, `src/index.txt`), `v2`);

        const changed = JSON.parse((await runSwitch(`tasks`, `hash`, `--json`, `build`, {env: {MY_VAR: `secret-value`}})).stdout.trim().split(`\n`).pop()!);
        expect(changed.fingerprint).not.toEqual(before.fingerprint);
        expect(changed.hasEntry).toEqual(false);

        // Hashing doesn't run anything
        expect(await readCounter(path, `prepare`)).toEqual(1);
        expect(await readCounter(path, `build`)).toEqual(1);
      })),
    );
    test(
      `it should cache the tasks of the @workspaces defaults across a workspace selection`,
      makeTemporaryMonorepoEnv({
        name: `root`,
        workspaces: [`packages/*`],
      }, {
        [`packages/pkg-a`]: {name: `pkg-a`, scripts: {build: `echo run >> "$COUNTERS/a" && mkdir -p dist && cp src/index.txt dist/index.txt && echo build-a`}},
        // No build script: a no-op that must still propagate fingerprints
        [`packages/pkg-b`]: {name: `pkg-b`, dependencies: {[`pkg-a`]: `workspace:*`}},
        [`packages/pkg-c`]: {name: `pkg-c`, scripts: {build: `echo run >> "$COUNTERS/c" && echo build-c`}, dependencies: {[`pkg-b`]: `workspace:*`}},
      }, cleanupDaemon(async ({path, run, runSwitch}) => {
        await setupProject(path);

        const env = {COUNTERS: npath.fromPortablePath(countersPath)};

        await xfs.mkdirPromise(ppath.join(path, `packages/pkg-a/src` as PortablePath));
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-a/src/index.txt` as PortablePath), `v1`);

        await writeTaskfile(path, [
          `@workspaces`,
          `@cache`,
          `@outputs(dist/**)`,
          `build: ^build`,
        ]);

        await run(`install`);

        const first = await runSwitch(`tasks`, `run`, `--standalone`, `-A`, `build`, {env});
        expect(first.stdout).toEqual(`[pkg-a:build]: build-a\n[pkg-c:build]: build-c\n`);

        const second = await runSwitch(`tasks`, `run`, `--standalone`, `-A`, `build`, {env});
        expect(second.stdout).toContain(`[pkg-a:build]: Cache hit, replaying output`);
        expect(second.stdout).toContain(`[pkg-c:build]: Cache hit, replaying output`);
        expect(await readCounter(path, `a`)).toEqual(1);
        expect(await readCounter(path, `c`)).toEqual(1);

        // The outputs get restored on hits
        await xfs.removePromise(ppath.join(path, `packages/pkg-a/dist` as PortablePath));
        await runSwitch(`tasks`, `run`, `--standalone`, `--errors-only`, `-A`, `build`, {env});
        expect(await xfs.readFilePromise(ppath.join(path, `packages/pkg-a/dist/index.txt` as PortablePath), `utf8`)).toEqual(`v1`);
        expect(await readCounter(path, `a`)).toEqual(1);

        // Changes cascade through the script-less pkg-b
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-a/src/index.txt` as PortablePath), `v2`);
        await runSwitch(`tasks`, `run`, `--standalone`, `-A`, `build`, {env});
        expect(await readCounter(path, `a`)).toEqual(2);
        expect(await readCounter(path, `c`)).toEqual(2);

        await runSwitch(`tasks`, `run`, `--standalone`, `--no-cache`, `-A`, `build`, {env});
        expect(await readCounter(path, `a`)).toEqual(3);
        expect(await readCounter(path, `c`)).toEqual(3);
      })),
    );
  });
});
