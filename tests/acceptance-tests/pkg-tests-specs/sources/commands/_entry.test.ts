import {PortablePath, npath, xfs} from '@yarnpkg/fslib';

describe(`Entry`, () => {
  describe(`--version`, () => {
    const versionPattern = /^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/;

    test(
      `it should print the binary version when given --version`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        const {stdout} = await run(`--version`);
        expect(stdout.trim()).toMatch(versionPattern);
      }),
    );

    test(
      `it should print the binary version when given -v`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        const {stdout} = await run(`-v`);
        expect(stdout.trim()).toMatch(versionPattern);
      }),
    );
  });

  describe(`--cwd`, () => {
    test(`should use the specified --cwd (relative path)`, makeTemporaryEnv({}, async ({path, run, source}) => {
      await run(`install`);

      await xfs.mkdirPromise(`${path}/packages` as PortablePath);

      await expect(run(`--cwd`, `packages`, `exec`, `pwd`)).resolves.toMatchObject({
        stdout: `${npath.fromPortablePath(`${path}/packages`)}\n`,
      });
    }));

    test(`should use the specified --cwd (absolute path)`, makeTemporaryEnv({}, async ({path, run, source}) => {
      await run(`install`);

      await xfs.mkdirPromise(`${path}/packages` as PortablePath);

      await expect(run(`--cwd`, npath.fromPortablePath(`${path}/packages`), `exec`, `pwd`)).resolves.toMatchObject({
        stdout: `${npath.fromPortablePath(`${path}/packages`)}\n`,
      });
    }));

    test(`should use the specified --cwd (relative symlink)`, makeTemporaryEnv({}, async ({path, run, source}) => {
      await run(`install`);

      await xfs.mkdirPromise(`${path}/packages` as PortablePath);
      await xfs.symlinkPromise(`${path}/packages` as PortablePath, `${path}/modules` as PortablePath);

      await expect(run(`--cwd`, `modules`, `exec`, `pwd`)).resolves.toMatchObject({
        stdout: `${npath.fromPortablePath(`${path}/modules`)}\n`,
      });
    }));

    test(`should use the specified --cwd (absolute symlink)`, makeTemporaryEnv({}, async ({path, run, source}) => {
      await run(`install`);

      await xfs.mkdirPromise(`${path}/packages` as PortablePath);
      await xfs.symlinkPromise(`${path}/packages` as PortablePath, `${path}/modules` as PortablePath);

      await expect(run(`--cwd`, npath.fromPortablePath(`${path}/modules`), `exec`, `pwd`)).resolves.toMatchObject({
        stdout: `${npath.fromPortablePath(`${path}/modules`)}\n`,
      });
    }));

    test(`should use the specified --cwd (bound relative path)`, makeTemporaryEnv({}, async ({path, run, source}) => {
      await run(`install`);

      await xfs.mkdirPromise(`${path}/packages` as PortablePath);

      await expect(run(`--cwd=packages`, `exec`, `pwd`)).resolves.toMatchObject({
        stdout: `${npath.fromPortablePath(`${path}/packages`)}\n`,
      });
    }));

    test(`should use the specified --cwd (bound absolute path)`, makeTemporaryEnv({}, async ({path, run, source}) => {
      await run(`install`);

      await xfs.mkdirPromise(`${path}/packages` as PortablePath);

      await expect(run(`--cwd=${npath.fromPortablePath(`${path}/packages`)}`, `exec`, `pwd`)).resolves.toMatchObject({
        stdout: `${npath.fromPortablePath(`${path}/packages`)}\n`,
      });
    }));

    test(`should use the specified --cwd (inside script)`, makeTemporaryEnv({
      scripts: {
        foo: `yarn --cwd=packages exec pwd`,
      },
    }, async ({path, run, source}) => {
      await run(`install`);

      await xfs.mkdirPromise(`${path}/packages` as PortablePath);

      await expect(run(`run`, `foo`)).resolves.toMatchObject({
        stdout: `${npath.fromPortablePath(`${path}/packages`)}\n`,
      });
    }));
  });
});
