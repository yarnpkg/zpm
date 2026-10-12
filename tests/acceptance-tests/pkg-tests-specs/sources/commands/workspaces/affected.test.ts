import {ppath, xfs, PortablePath} from '@yarnpkg/fslib';
import {execFileSync}            from 'child_process';

// a <- b <- c ; d independent
const affectedEnv = (cb: (args: {path: PortablePath, run: any, git: (...args: Array<string>) => string}) => Promise<void>) => makeTemporaryMonorepoEnv({
  name: `root`,
  workspaces: [`packages/*`],
}, {
  [`packages/pkg-a`]: {name: `pkg-a`},
  [`packages/pkg-b`]: {name: `pkg-b`, dependencies: {[`pkg-a`]: `workspace:*`}},
  [`packages/pkg-c`]: {name: `pkg-c`, devDependencies: {[`pkg-b`]: `workspace:*`}},
  [`packages/pkg-d`]: {name: `pkg-d`},
}, async ({path, run}) => {
  await run(`install`);

  const git = (...args: Array<string>) => execFileSync(`git`, args, {cwd: npathOf(path), encoding: `utf8`}).trim();

  await xfs.writeFilePromise(ppath.join(path, `.gitignore` as PortablePath), `.yarn\n.pnp.*\n`);

  git(`init`, `-q`, `-b`, `main`);
  git(`config`, `user.name`, `John Doe`);
  git(`config`, `user.email`, `john.doe@example.org`);
  git(`config`, `commit.gpgSign`, `false`);
  git(`add`, `-A`);
  git(`commit`, `-q`, `-m`, `First commit`);

  await cb({path, run, git});
});

const npathOf = (path: PortablePath) => require(`@yarnpkg/fslib`).npath.fromPortablePath(path);

const names = (stdout: string) => stdout.trim().split(`\n`).filter(Boolean).map(line => JSON.parse(line).name).sort();

describe(`Commands`, () => {
  describe(`workspaces list --since (affected detection)`, () => {
    test(
      `--since -R should return the changed workspaces and their transitive dependents`,
      affectedEnv(async ({path, run}) => {
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-a/index.js` as PortablePath), `// change\n`);

        const {stdout} = await run(`workspaces`, `list`, `--since`, `-R`, `--json`);
        expect(names(stdout)).toEqual([`pkg-a`, `pkg-b`, `pkg-c`]);
      }),
    );

    test(
      `changes to changesetGlobalFiles should mark every workspace as changed`,
      affectedEnv(async ({path, run, git}) => {
        await xfs.writeFilePromise(ppath.join(path, `.yarnrc.yml` as PortablePath), `changesetGlobalFiles:\n  - .nvmrc\n  - patches/**\n`);
        git(`add`, `-A`);
        git(`commit`, `-q`, `-m`, `config`);

        await xfs.mkdirPromise(ppath.join(path, `patches` as PortablePath));
        await xfs.writeFilePromise(ppath.join(path, `patches/foo.patch` as PortablePath), `patch\n`);

        const {stdout} = await run(`workspaces`, `list`, `--since`, `--json`);
        expect(names(stdout)).toEqual([`pkg-a`, `pkg-b`, `pkg-c`, `pkg-d`, `root`]);
      }),
    );

    test(
      `files outside of changesetGlobalFiles and outside workspaces should only affect the root workspace`,
      affectedEnv(async ({path, run, git}) => {
        await xfs.writeFilePromise(ppath.join(path, `.yarnrc.yml` as PortablePath), `changesetGlobalFiles:\n  - .nvmrc\n`);
        git(`add`, `-A`);
        git(`commit`, `-q`, `-m`, `config`);

        await xfs.writeFilePromise(ppath.join(path, `README.md` as PortablePath), `hello\n`);

        const {stdout} = await run(`workspaces`, `list`, `--since`, `--json`);
        expect(names(stdout)).toEqual([`root`]);
      }),
    );

    test(
      `--head should compare two refs, ignoring the working tree`,
      affectedEnv(async ({path, run, git}) => {
        const base = git(`rev-parse`, `HEAD`);

        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-b/index.js` as PortablePath), `// change\n`);
        git(`add`, `-A`);
        git(`commit`, `-q`, `-m`, `change b`);
        const head = git(`rev-parse`, `HEAD`);

        // Uncommitted change that must be ignored when --head is set
        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-d/index.js` as PortablePath), `// change\n`);

        const {stdout} = await run(`workspaces`, `list`, `--since=${base}`, `--head=${head}`, `-R`, `--json`);
        expect(names(stdout)).toEqual([`pkg-b`, `pkg-c`]);

        const {stdout: worktree} = await run(`workspaces`, `list`, `--since=${base}`, `-R`, `--json`);
        expect(names(worktree)).toEqual([`pkg-b`, `pkg-c`, `pkg-d`]);
      }),
    );

    test(
      `TURBO_SCM_BASE / TURBO_SCM_HEAD should be honored when no explicit ref is given`,
      affectedEnv(async ({path, run, git}) => {
        const base = git(`rev-parse`, `HEAD`);

        await xfs.writeFilePromise(ppath.join(path, `packages/pkg-d/index.js` as PortablePath), `// change\n`);
        git(`add`, `-A`);
        git(`commit`, `-q`, `-m`, `change d`);

        // On main, comparing against the base refs would yield nothing
        const {stdout: empty} = await run(`workspaces`, `list`, `--since`, `-R`, `--json`);
        expect(names(empty)).toEqual([]);

        const {stdout} = await run(`workspaces`, `list`, `--since`, `-R`, `--json`, {env: {TURBO_SCM_BASE: base, TURBO_SCM_HEAD: `HEAD`}});
        expect(names(stdout)).toEqual([`pkg-d`]);

        const {stdout: yarnVars} = await run(`workspaces`, `list`, `--since`, `-R`, `--json`, {env: {YARN_CHANGESET_BASE: base}});
        expect(names(yarnVars)).toEqual([`pkg-d`]);
      }),
    );

    test(
      `lockfile changes should only mark workspaces whose dependency tree changed`,
      affectedEnv(async ({path, run, git}) => {
        const manifestPath = ppath.join(path, `packages/pkg-d/package.json` as PortablePath);
        const manifest = await xfs.readJsonPromise(manifestPath);

        // Adding a dependency changes pkg-d's manifest and the lockfile
        await xfs.writeJsonPromise(manifestPath, {...manifest, dependencies: {[`pkg-a`]: `workspace:*`}});
        await run(`install`);
        git(`add`, `-A`);
        git(`commit`, `-q`, `-m`, `deps`);

        const {stdout} = await run(`workspaces`, `list`, `--since=HEAD~1`, `-R`, `--json`);
        expect(names(stdout)).toEqual([`pkg-d`]);
      }),
    );
  });
});
