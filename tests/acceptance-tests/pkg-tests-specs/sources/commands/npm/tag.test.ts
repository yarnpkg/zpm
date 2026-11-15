import {ppath, xfs} from '@yarnpkg/fslib';

const {
  misc,
  tests: {RequestType, startPackageServer, startRegistryRecording, validLogins},
} = require(`pkg-tests-core`);

// Changes made through the dist-tags endpoints of the mock registry are
// tracked separately for each registry path, so tests that modify tags use
// their own registry to stay independent from each other.
async function getIsolatedRegistryEnv(name: string, login = validLogins.fooUser) {
  const registry = `${await startPackageServer()}/registry/${name}`;

  return {
    YARN_NPM_REGISTRY_SERVER: registry,
    YARN_NPM_PUBLISH_REGISTRY: registry,
    YARN_NPM_AUTH_TOKEN: login.npmAuthToken,
  };
}

describe(`Commands`, () => {
  describe(`npm tag list`, () => {
    test(
      `it should list all tags of a package`,
      makeTemporaryEnv({}, async ({run}) => {
        const {stdout} = await run(`npm`, `tag`, `list`, `no-deps-tags`);

        expect(stdout).toContain(`no-deps-tags@npm:1.0.0`);
        expect(stdout).toContain(`no-deps-tags@npm:1.0.0-rc.1`);
      }),
    );

    test(
      `it should list all tags of a package as NDJSON, sorted by tag name`,
      makeTemporaryEnv({}, async ({run}) => {
        const {stdout} = await run(`npm`, `tag`, `list`, `no-deps-tags`, `--json`);

        expect(misc.parseJsonStream(stdout)).toEqual([
          {descriptor: `no-deps-tags@latest`, locator: `no-deps-tags@npm:1.0.0`},
          {descriptor: `no-deps-tags@rc`, locator: `no-deps-tags@npm:1.0.0-rc.1`},
        ]);
      }),
    );

    test(
      `it should default to the current workspace`,
      makeTemporaryEnv({
        name: `no-deps-tags`,
        version: `1.0.0`,
      }, async ({run}) => {
        const {stdout} = await run(`npm`, `tag`, `list`, `--json`);

        expect(misc.parseJsonStream(stdout)).toEqual([
          {descriptor: `no-deps-tags@latest`, locator: `no-deps-tags@npm:1.0.0`},
          {descriptor: `no-deps-tags@rc`, locator: `no-deps-tags@npm:1.0.0-rc.1`},
        ]);
      }),
    );

    test(
      `it should require the current workspace to have a name when no package is specified`,
      makeTemporaryEnv({}, async ({run}) => {
        await expect(run(`npm`, `tag`, `list`)).rejects.toThrow(/Missing 'name' field in .*package\.json/);
      }),
    );

    test(
      `it should list the tags of scoped packages from the scope's registry`,
      makeTemporaryEnv({}, async ({path, run}) => {
        const registry = await startPackageServer();

        await xfs.writeJsonPromise(ppath.join(path, `.yarnrc.yml`), {
          npmScopes: {
            scoped: {
              npmRegistryServer: `${registry}/registry/scope`,
            },
          },
        });

        const requests = await startRegistryRecording(async () => {
          const {stdout} = await run(`npm`, `tag`, `list`, `@scoped/no-deps`, `--json`);

          expect(misc.parseJsonStream(stdout)).toEqual([
            {descriptor: `@scoped/no-deps@latest`, locator: `@scoped/no-deps@npm:2.0.0`},
          ]);
        });

        expect(requests).toEqual([
          {type: RequestType.DistTagList, registry: `scope`, scope: `@scoped`, localName: `no-deps`},
        ]);
      }),
    );

    test(
      `it should authenticate requests for private scoped packages`,
      makeTemporaryEnv({}, async ({run}) => {
        await expect(run(`npm`, `tag`, `list`, `@private/package`)).rejects.toThrow();

        const {stdout} = await run(`npm`, `tag`, `list`, `@private/package`, `--json`, {
          env: {
            YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
          },
        });

        expect(misc.parseJsonStream(stdout)).toEqual([
          {descriptor: `@private/package@latest`, locator: `@private/package@npm:1.0.0`},
        ]);
      }),
    );

    test(
      `it should not require an OTP to list tags`,
      makeTemporaryEnv({}, async ({run}) => {
        const {stdout} = await run(`npm`, `tag`, `list`, `@scoped/no-deps`, `--json`, {
          env: {
            YARN_NPM_AUTH_TOKEN: validLogins.otpUser.npmAuthToken,
          },
        });

        expect(misc.parseJsonStream(stdout)).toEqual([
          {descriptor: `@scoped/no-deps@latest`, locator: `@scoped/no-deps@npm:2.0.0`},
        ]);
      }),
    );

    test(
      `it should report packages that don't exist`,
      makeTemporaryEnv({}, async ({run}) => {
        await expect(run(`npm`, `tag`, `list`, `does-not-exist`)).rejects.toThrow(/Package not found/);
      }),
    );

    test(
      `it should reject invalid package names`,
      makeTemporaryEnv({}, async ({run}) => {
        await expect(run(`npm`, `tag`, `list`, `no-deps@1.0.0`)).rejects.toThrow(/Invalid ident/);
      }),
    );
  });

  describe(`npm tag add`, () => {
    test(
      `it should add a tag to a specific version of a package`,
      makeTemporaryEnv({}, async ({run}) => {
        const env = await getIsolatedRegistryEnv(`tagadd`);

        const requests = await startRegistryRecording(async () => {
          const {stdout} = await run(`npm`, `tag`, `add`, `no-deps@1.0.0`, `beta`, {env});
          expect(stdout).toContain(`Tag beta added to version 1.0.0 of package no-deps`);
        });

        expect(requests).toEqual([
          {type: RequestType.DistTagList, registry: `tagadd`, localName: `no-deps`},
          {type: RequestType.DistTagAdd, registry: `tagadd`, localName: `no-deps`, tag: `beta`},
        ]);

        const {stdout} = await run(`npm`, `tag`, `list`, `no-deps`, `--json`, {env});
        expect(misc.parseJsonStream(stdout)).toEqual([
          {descriptor: `no-deps@beta`, locator: `no-deps@npm:1.0.0`},
          {descriptor: `no-deps@latest`, locator: `no-deps@npm:2.0.0`},
        ]);
      }),
    );

    test(
      `it should overwrite an existing tag`,
      makeTemporaryEnv({}, async ({run}) => {
        const env = await getIsolatedRegistryEnv(`tagoverwrite`);

        await run(`npm`, `tag`, `add`, `no-deps-tags@1.0.0`, `rc`, {env});

        const {stdout} = await run(`npm`, `tag`, `list`, `no-deps-tags`, `--json`, {env});
        expect(misc.parseJsonStream(stdout)).toEqual([
          {descriptor: `no-deps-tags@latest`, locator: `no-deps-tags@npm:1.0.0`},
          {descriptor: `no-deps-tags@rc`, locator: `no-deps-tags@npm:1.0.0`},
        ]);
      }),
    );

    test(
      `it should warn when the tag already points to the requested version`,
      makeTemporaryEnv({}, async ({run}) => {
        const env = await getIsolatedRegistryEnv(`tagalreadyset`);

        const requests = await startRegistryRecording(async () => {
          const {stdout} = await run(`npm`, `tag`, `add`, `no-deps@2.0.0`, `latest`, {env});
          expect(stdout).toContain(`Tag latest is already set to version 2.0.0`);
          expect(stdout).toContain(`Tag latest added to version 2.0.0 of package no-deps`);
        });

        expect(requests).toContainEqual({type: RequestType.DistTagAdd, registry: `tagalreadyset`, localName: `no-deps`, tag: `latest`});
      }),
    );

    test(
      `it should add tags to scoped packages`,
      makeTemporaryEnv({}, async ({run}) => {
        const env = await getIsolatedRegistryEnv(`tagaddscoped`);

        const requests = await startRegistryRecording(async () => {
          await run(`npm`, `tag`, `add`, `@scoped/no-deps@1.0.1`, `next`, {env});
        });

        expect(requests).toEqual([
          {type: RequestType.DistTagList, registry: `tagaddscoped`, scope: `@scoped`, localName: `no-deps`},
          {type: RequestType.DistTagAdd, registry: `tagaddscoped`, scope: `@scoped`, localName: `no-deps`, tag: `next`},
        ]);

        const {stdout} = await run(`npm`, `tag`, `list`, `@scoped/no-deps`, `--json`, {env});
        expect(misc.parseJsonStream(stdout)).toEqual([
          {descriptor: `@scoped/no-deps@latest`, locator: `@scoped/no-deps@npm:2.0.0`},
          {descriptor: `@scoped/no-deps@next`, locator: `@scoped/no-deps@npm:1.0.1`},
        ]);
      }),
    );

    for (const registrySource of [`manifest`, `scope`, `package`]) {
      test(
        `it should use the ${registrySource} publish registry and credentials`,
        makeTemporaryEnv({
          private: true,
          workspaces: [`packages/*`],
        }, async ({path, run}) => {
          const registry = await startPackageServer();
          const label = `tag${registrySource}`;

          await xfs.mkdirpPromise(ppath.join(path, `packages/pkg`));
          await xfs.writeJsonPromise(ppath.join(path, `packages/pkg/package.json`), {
            name: `@scoped/no-deps`,
            version: `1.0.0`,
            ...(registrySource === `manifest` && {
              publishConfig: {registry: `${registry}/registry/${label}`},
            }),
          });

          await xfs.writeJsonPromise(ppath.join(path, `.yarnrc.yml`), {
            npmPublishRegistry: `${registry}/registry/default`,
            npmScopes: {
              scoped: {
                npmPublishRegistry: registrySource === `scope` ? `${registry}/registry/${label}` : `${registry}/registry/scope`,
                ...(registrySource === `scope` && {npmAuthToken: validLogins.fooUser.npmAuthToken}),
              },
            },
            ...(registrySource === `package` && {
              packageRules: [{
                packageFilter: `@scoped/*`,
                npmPublishRegistry: `${registry}/registry/${label}`,
                npmAuthToken: validLogins.fooUser.npmAuthToken,
              }],
            }),
            ...(registrySource === `manifest` && {
              sourceRules: [{
                registryFilter: `${registry}/registry/${label}`,
                npmAuthToken: validLogins.fooUser.npmAuthToken,
              }],
            }),
          });

          // The selected registry's credentials must override the unusable default ones.
          // The command runs from the project root to check that the registry is selected
          // from the package being tagged, not from the active workspace.
          const env = {YARN_NPM_AUTH_TOKEN: `invalid-token`};

          const requests = await startRegistryRecording(async () => {
            await run(`npm`, `tag`, `add`, `@scoped/no-deps@1.0.0`, `beta`, {env});
            await run(`npm`, `tag`, `remove`, `@scoped/no-deps`, `beta`, {env});
          });

          expect(requests).toEqual([
            {type: RequestType.DistTagList, registry: label, scope: `@scoped`, localName: `no-deps`},
            {type: RequestType.DistTagAdd, registry: label, scope: `@scoped`, localName: `no-deps`, tag: `beta`},
            {type: RequestType.DistTagList, registry: label, scope: `@scoped`, localName: `no-deps`},
            {type: RequestType.DistTagRemove, registry: label, scope: `@scoped`, localName: `no-deps`, tag: `beta`},
          ]);
        }),
      );
    }

    test(
      `it should support OTP`,
      makeTemporaryEnv({}, async ({run}) => {
        const env = await getIsolatedRegistryEnv(`tagaddotp`, validLogins.otpUser);

        await expect(run(`npm`, `tag`, `add`, `no-deps@1.0.0`, `beta`, `--otp`, `invalid_otp`, {env})).rejects.toThrow(/Invalid OTP token/);

        const {stdout} = await run(`npm`, `tag`, `add`, `no-deps@1.0.0`, `beta`, `--otp`, validLogins.otpUser.npmOtpToken, {env});
        expect(stdout).toContain(`Tag beta added to version 1.0.0 of package no-deps`);

        const {stdout: injectedStdout} = await run(`npm`, `tag`, `add`, `no-deps@1.0.1`, `beta`, {
          env: {...env, YARN_INJECT_NPM_2FA_TOKEN: validLogins.otpUser.npmOtpToken},
        });
        expect(injectedStdout).toContain(`Tag beta added to version 1.0.1 of package no-deps`);
      }),
    );

    test(
      `it should require authentication`,
      makeTemporaryEnv({}, async ({run}) => {
        const requests = await startRegistryRecording(async () => {
          await expect(run(`npm`, `tag`, `add`, `no-deps@1.0.0`, `beta`)).rejects.toThrow(/No authentication configured for request/);
        });

        expect(requests).toEqual([]);
      }),
    );

    test(
      `it should require a valid semver version`,
      makeTemporaryEnv({}, async ({run}) => {
        const env = await getIsolatedRegistryEnv(`taginvalid`);

        const requests = await startRegistryRecording(async () => {
          await expect(run(`npm`, `tag`, `add`, `no-deps@^1.0.0`, `beta`, {env})).rejects.toThrow(/The range \^1\.0\.0 must be a valid semver version/);
          await expect(run(`npm`, `tag`, `add`, `no-deps@beta`, `beta`, {env})).rejects.toThrow(/must be a valid semver version/);
          await expect(run(`npm`, `tag`, `add`, `no-deps`, `beta`, {env})).rejects.toThrow(/Invalid package version \(no-deps\); expected <name>@<version>/);
        });

        expect(requests).toEqual([]);
      }),
    );

    for (const [index, version] of [`1`, `1.0`, `*`, `1.x`, `1.0.x`, `=1.0.0`, `=1`].entries()) {
      test(
        `it should reject ranges, incomplete versions and wildcards (${version})`,
        makeTemporaryEnv({}, async ({run}) => {
          const env = await getIsolatedRegistryEnv(`tagincomplete${String.fromCharCode(97 + index)}`);

          const requests = await startRegistryRecording(async () => {
            await expect(run(`npm`, `tag`, `add`, `no-deps@${version}`, `beta`, {env})).rejects.toThrow(/must be a valid semver version/);
          });

          expect(requests).toEqual([]);

          const {stdout} = await run(`npm`, `tag`, `list`, `no-deps`, `--json`, {env});
          expect(misc.parseJsonStream(stdout)).toEqual([
            {descriptor: `no-deps@latest`, locator: `no-deps@npm:2.0.0`},
          ]);
        }),
      );
    }

    test(
      `it should ignore build metadata, as registries don't key versions with it`,
      makeTemporaryEnv({}, async ({run}) => {
        const env = await getIsolatedRegistryEnv(`tagbuildmetadata`);

        const {stdout: addStdout} = await run(`npm`, `tag`, `add`, `no-deps@1.0.0+build.1`, `beta`, {env});
        expect(addStdout).toContain(`Tag beta added to version 1.0.0 of package no-deps`);

        const {stdout} = await run(`npm`, `tag`, `list`, `no-deps`, `--json`, {env});
        expect(misc.parseJsonStream(stdout)).toEqual([
          {descriptor: `no-deps@beta`, locator: `no-deps@npm:1.0.0`},
          {descriptor: `no-deps@latest`, locator: `no-deps@npm:2.0.0`},
        ]);
      }),
    );

    test(
      `it should report versions that don't exist`,
      makeTemporaryEnv({}, async ({run}) => {
        const env = await getIsolatedRegistryEnv(`tagmissingversion`);

        await expect(run(`npm`, `tag`, `add`, `no-deps@9.9.9`, `beta`, {env})).rejects.toThrow(/Version not found: no-deps@9\.9\.9/);
      }),
    );

    test(
      `it should report packages that don't exist`,
      makeTemporaryEnv({}, async ({run}) => {
        const env = await getIsolatedRegistryEnv(`tagmissingpackage`);

        const requests = await startRegistryRecording(async () => {
          await expect(run(`npm`, `tag`, `add`, `does-not-exist@1.0.0`, `beta`, {env})).rejects.toThrow(/Package not found/);
        });

        expect(requests).toEqual([
          {type: RequestType.DistTagList, registry: `tagmissingpackage`, localName: `does-not-exist`},
        ]);
      }),
    );
  });

  describe(`npm tag remove`, () => {
    test(
      `it should remove a tag from a package`,
      makeTemporaryEnv({}, async ({run}) => {
        const env = await getIsolatedRegistryEnv(`tagremove`);

        const requests = await startRegistryRecording(async () => {
          const {stdout} = await run(`npm`, `tag`, `remove`, `no-deps-tags`, `rc`, {env});
          expect(stdout).toContain(`Tag rc removed from package no-deps-tags`);
        });

        expect(requests).toEqual([
          {type: RequestType.DistTagList, registry: `tagremove`, localName: `no-deps-tags`},
          {type: RequestType.DistTagRemove, registry: `tagremove`, localName: `no-deps-tags`, tag: `rc`},
        ]);

        const {stdout} = await run(`npm`, `tag`, `list`, `no-deps-tags`, `--json`, {env});
        expect(misc.parseJsonStream(stdout)).toEqual([
          {descriptor: `no-deps-tags@latest`, locator: `no-deps-tags@npm:1.0.0`},
        ]);
      }),
    );

    test(
      `it should remove a tag from a scoped package with OTP`,
      makeTemporaryEnv({}, async ({run}) => {
        const env = await getIsolatedRegistryEnv(`tagremovescoped`, validLogins.otpUser);

        await run(`npm`, `tag`, `add`, `@scoped/no-deps@1.0.0`, `beta`, `--otp`, validLogins.otpUser.npmOtpToken, {env});

        await expect(run(`npm`, `tag`, `remove`, `@scoped/no-deps`, `beta`, `--otp`, `invalid_otp`, {env})).rejects.toThrow(/Invalid OTP token/);

        const requests = await startRegistryRecording(async () => {
          const {stdout} = await run(`npm`, `tag`, `remove`, `@scoped/no-deps`, `beta`, `--otp`, validLogins.otpUser.npmOtpToken, {env});
          expect(stdout).toContain(`Tag beta removed from package @scoped/no-deps`);
        });

        expect(requests).toContainEqual({type: RequestType.DistTagRemove, registry: `tagremovescoped`, scope: `@scoped`, localName: `no-deps`, tag: `beta`});

        const {stdout} = await run(`npm`, `tag`, `list`, `@scoped/no-deps`, `--json`, {env});
        expect(misc.parseJsonStream(stdout)).toEqual([
          {descriptor: `@scoped/no-deps@latest`, locator: `@scoped/no-deps@npm:2.0.0`},
        ]);
      }),
    );

    test(
      `it should refuse to remove the latest tag`,
      makeTemporaryEnv({}, async ({run}) => {
        const env = await getIsolatedRegistryEnv(`tagremovelatest`);

        const requests = await startRegistryRecording(async () => {
          await expect(run(`npm`, `tag`, `remove`, `no-deps`, `latest`, {env})).rejects.toThrow(/The 'latest' tag cannot be removed\./);
        });

        expect(requests).toEqual([]);
      }),
    );

    test(
      `it should report tags that don't exist`,
      makeTemporaryEnv({}, async ({run}) => {
        const env = await getIsolatedRegistryEnv(`tagremoveunknown`);

        const requests = await startRegistryRecording(async () => {
          await expect(run(`npm`, `tag`, `remove`, `no-deps`, `beta`, {env})).rejects.toThrow(/beta is not a tag of package no-deps/);
        });

        expect(requests).toEqual([
          {type: RequestType.DistTagList, registry: `tagremoveunknown`, localName: `no-deps`},
        ]);
      }),
    );

    test(
      `it should report packages that don't exist`,
      makeTemporaryEnv({}, async ({run}) => {
        const env = await getIsolatedRegistryEnv(`tagremovemissing`);

        await expect(run(`npm`, `tag`, `remove`, `does-not-exist`, `beta`, {env})).rejects.toThrow(/Package not found/);
      }),
    );

    test(
      `it should require authentication`,
      makeTemporaryEnv({}, async ({run}) => {
        await expect(run(`npm`, `tag`, `remove`, `no-deps`, `beta`)).rejects.toThrow(/No authentication configured for request/);
      }),
    );
  });
});
