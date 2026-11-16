const {
  fs: {writeFile, writeJson},
  tests: {startPackageServer},
} = require(`pkg-tests-core`);

describe(`Features`, () => {
  describe(`npmMinimalAgeGate and npmPreapprovedPackages`, () => {
    describe(`add`, () => {
      test(
        `add should install the latest version allowed by the minimum release age`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: `1d`,
        }, async ({run, source}) => {
          await run(`add`, `release-date`);
          await expect(source(`require('release-date/package.json')`)).resolves.toMatchObject({
            name: `release-date`,
            version: `1.1.0`,
          });
        }),
      );

      test(
        `it should fail when trying to install exact version that is newer than the minimum release age`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: `1d`,
        }, async ({run}) => {
          await expect(run(`add`, `release-date@1.1.1`)).rejects.toThrowError(`No candidates found`);
        }),
      );

      test(
        `it should install older package versions when the minimum release age disallows the newest suitable version`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: `1d`,
        }, async ({run, source}) => {
          await run(`add`, `release-date@^1.0.0`);

          await expect(source(`require('release-date/package.json')`)).resolves.toMatchObject({
            name: `release-date`,
            version: `1.1.0`,
          });
        }),
      );

      test(
        `it should install new version when excluded by a descriptor; while transitive dependencies are not excluded`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: `1d`,
          npmPreapprovedPackages: [`release-date@^1.0.0`],
        }, async ({run, source}) => {
          await run(`add`, `release-date@^1.0.0`);

          await expect(source(`require('release-date')`)).resolves.toMatchObject({
            name: `release-date`,
            version: `1.1.1`,
            dependencies: {
              [`release-date-transitive`]: {
                name: `release-date-transitive`,
                version: `1.1.0`,
              },
            },
          });
        }),
      );

      test(
        `it should install new version when excluded by package ident`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: `1d`,
          npmPreapprovedPackages: [`release-date`],
        }, async ({run, source}) => {
          await run(`add`, `release-date@^1.0.0`);

          await expect(source(`require('release-date/package.json')`)).resolves.toMatchObject({
            name: `release-date`,
            version: `1.1.1`,
          });
        }),
      );

      test(
        `it should not impact semver prioritization of newer versions when multiple versions meet the age requirement`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: 0,
        }, async ({run, source}) => {
          await run(`add`, `release-date@^1.0.0`);

          await expect(source(`require('release-date/package.json')`)).resolves.toMatchObject({
            name: `release-date`,
            version: `1.1.1`,
          });
        }),
      );

      test(
        `it should work with scoped packages`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: `1d`,
        }, async ({run}) => {
          await expect(run(`add`, `@scoped/release-date@1.1.1`)).rejects.toThrowError(`No candidates found`);
        }),
      );

      test(
        `it should install scoped package when excluded`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: `1d`,
          npmPreapprovedPackages: [`@scoped/release-date`],
        }, async ({run, source}) => {
          await run(`add`, `@scoped/release-date@^1.0.0`);

          await expect(source(`require('@scoped/release-date/package.json')`)).resolves.toMatchObject({
            name: `@scoped/release-date`,
            version: `1.1.2`,
          });
        }),
      );

      test(
        `it should install scoped package when excluded by scoped glob pattern`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: `1d`,
          npmPreapprovedPackages: [`@scoped/*`],
        }, async ({run, source}) => {
          await run(`add`, `@scoped/release-date@^1.0.0`);

          await expect(source(`require('@scoped/release-date/package.json')`)).resolves.toMatchObject({
            name: `@scoped/release-date`,
            version: `1.1.2`,
          });
        }),
      );

      test(
        `it should not install a version via add that is higher than the latest tag`,
        makeTemporaryEnv({
        }, {
          npmMinimalAgeGate: `1d`,
        }, async ({run, source}) => {
          await run(`add`, `@scoped/release-date`);

          await expect(source(`require('@scoped/release-date/package.json')`)).resolves.toMatchObject({
            name: `@scoped/release-date`,
            version: `1.1.0`,
          });
        }),
      );
    });
    describe(`install`, () => {
      test(
        `it should parse numeric npmMinimalAgeGate from .yarnrc.yml`,
        makeTemporaryEnv({
          dependencies: {[`release-date`]: `^1.0.0`},
        }, async ({path, run, source}) => {
          await writeFile(`${path}/.yarnrc.yml`, `npmMinimalAgeGate: 0\n`);

          await run(`install`);

          await expect(source(`require('release-date/package.json')`)).resolves.toMatchObject({
            name: `release-date`,
            version: `1.1.1`,
          });
        }),
      );

      test(
        `it should fail when trying to install exact version that is too new`,
        makeTemporaryEnv({
          dependencies: {[`release-date`]: `1.1.1`},
        }, {
          npmMinimalAgeGate: `1d`,
        }, async ({run}) => {
          await expect(run(`install`)).rejects.toThrowError(`No candidates found`);
        }),
      );

      test(
        `it should install older package versions when the minimum release age disallows the newest suitable version`,
        makeTemporaryEnv({
          dependencies: {[`release-date`]: `^1.0.0`},
        }, {
          npmMinimalAgeGate: `1d`,
        }, async ({run, source}) => {
          await run(`install`);

          await expect(source(`require('release-date/package.json')`)).resolves.toMatchObject({
            name: `release-date`,
            version: `1.1.0`,
          });
        }),
      );

      test(
        `it should install new version when excluded by a descriptor`,
        makeTemporaryEnv({
          dependencies: {[`release-date`]: `^1.0.0`},
        }, {
          npmMinimalAgeGate: `1d`,
          npmPreapprovedPackages: [`release-date@^1.0.0`],
        }, async ({run, source}) => {
          await run(`install`);

          await expect(source(`require('release-date/package.json')`)).resolves.toMatchObject({
            name: `release-date`,
            version: `1.1.1`,
          });
        }),
      );

      test(
        `it should install new version when excluded by package name glob pattern`,
        makeTemporaryEnv({
          dependencies: {[`release-date`]: `^1.0.0`},
        }, {
          npmMinimalAgeGate: `1d`,
          npmPreapprovedPackages: [`release-*`],
        }, async ({run, source}) => {
          await run(`install`);

          await expect(source(`require('release-date/package.json')`)).resolves.toMatchObject({
            name: `release-date`,
            version: `1.1.1`,
          });
        }),
      );

      test(
        `it should install new version when excluded by package ident`,
        makeTemporaryEnv({
          dependencies: {[`release-date`]: `^1.0.0`},
        }, {
          npmMinimalAgeGate: `1d`,
          npmPreapprovedPackages: [`release-date`],
        }, async ({run, source}) => {
          await run(`install`);

          await expect(source(`require('release-date/package.json')`)).resolves.toMatchObject({
            name: `release-date`,
            version: `1.1.1`,
          });
        }),
      );

      test(
        `it should not impact semver prioritization of newer versions when multiple versions meet the age requirement`,
        makeTemporaryEnv({
          dependencies: {[`release-date`]: `^1.0.0`},
        }, {
          npmMinimalAgeGate: 0,
        }, async ({run, source}) => {
          await run(`install`);

          await expect(source(`require('release-date/package.json')`)).resolves.toMatchObject({
            name: `release-date`,
            version: `1.1.1`,
          });
        }),
      );

      test(
        `it should allow package rules to override the minimum release age`,
        makeTemporaryEnv({
          dependencies: {[`release-date`]: `1.1.1`},
        }, {
          npmMinimalAgeGate: `1d`,
        }, async ({run, source}) => {
          await run(`config`, `set`, `packageRules`, `--json`, JSON.stringify([{
            packageFilter: `release-date`,
            npmMinimalAgeGate: `0`,
          }]));

          await run(`install`);

          await expect(source(`require('release-date/package.json')`)).resolves.toMatchObject({
            name: `release-date`,
            version: `1.1.1`,
          });
        }),
      );

      test(
        `it should allow source rules to override the minimum release age`,
        makeTemporaryEnv({
          dependencies: {[`release-date`]: `1.1.1`},
        }, {
          npmMinimalAgeGate: `1d`,
        }, async ({run, source}) => {
          await run(`config`, `set`, `sourceRules`, `--json`, JSON.stringify([{
            ecosystemFilter: `npm`,
            npmMinimalAgeGate: `0`,
          }]));

          await run(`install`);

          await expect(source(`require('release-date/package.json')`)).resolves.toMatchObject({
            name: `release-date`,
            version: `1.1.1`,
          });
        }),
      );

      test(
        `it should allow source rules with a registry filter written without the protocol to override the minimum release age`,
        makeTemporaryEnv({
          dependencies: {[`release-date`]: `1.1.1`},
        }, {
          npmMinimalAgeGate: `1d`,
        }, async ({run, source}) => {
          const url = await startPackageServer();

          await run(`config`, `set`, `sourceRules`, `--json`, JSON.stringify([{
            registryFilter: `${url.replace(/^https?:/, ``)}/`,
            npmMinimalAgeGate: `0`,
          }]));

          await run(`install`);

          await expect(source(`require('release-date/package.json')`)).resolves.toMatchObject({
            name: `release-date`,
            version: `1.1.1`,
          });
        }),
      );

      test(
        `it shouldn't apply source rules with a registry filter written without the protocol to other registries`,
        makeTemporaryEnv({
          dependencies: {[`release-date`]: `1.1.1`},
        }, {
          npmMinimalAgeGate: `1d`,
        }, async ({run}) => {
          const url = await startPackageServer();

          await run(`config`, `set`, `sourceRules`, `--json`, JSON.stringify([{
            registryFilter: `${url.replace(/^https?:/, ``)}/other-registry`,
            npmMinimalAgeGate: `0`,
          }, {
            registryFilter: `//registry.example.org`,
            npmMinimalAgeGate: `0`,
          }]));

          await expect(run(`install`)).rejects.toThrowError(`No candidates found`);
        }),
      );

      test(
        `it should inherit the global minimum release age in rules that don't set it`,
        makeTemporaryEnv({
          dependencies: {[`@scoped/release-date`]: `^1.0.0`},
        }, {
          npmMinimalAgeGate: 0,
        }, async ({run, source}) => {
          await run(`config`, `set`, `packageRules`, `--json`, JSON.stringify([{
            packageFilter: `@scoped/*`,
            npmAlwaysAuth: false,
          }]));

          await run(`install`);

          await expect(source(`require('@scoped/release-date/package.json')`)).resolves.toMatchObject({
            name: `@scoped/release-date`,
            version: `1.1.2`,
          });
        }),
      );

      test(
        `it should inherit the global minimum release age in source rules that don't set it`,
        makeTemporaryEnv({
          dependencies: {[`@scoped/release-date`]: `^1.0.0`},
        }, {
          npmMinimalAgeGate: 0,
        }, async ({run, source}) => {
          await run(`config`, `set`, `sourceRules`, `--json`, JSON.stringify([{
            ecosystemFilter: `npm`,
            npmAlwaysAuth: false,
          }]));

          await run(`install`);

          await expect(source(`require('@scoped/release-date/package.json')`)).resolves.toMatchObject({
            name: `@scoped/release-date`,
            version: `1.1.2`,
          });
        }),
      );

      test(
        `it should work with scoped packages`,
        makeTemporaryEnv({
          dependencies: {[`@scoped/release-date`]: `1.1.1`},
        }, {
          npmMinimalAgeGate: `1d`,
        }, async ({run}) => {
          await expect(run(`install`)).rejects.toThrowError(`No candidates found`);
        }),
      );

      test(
        `it should install scoped package when excluded`,
        makeTemporaryEnv({
          dependencies: {[`@scoped/release-date`]: `^1.0.0`},
        }, {
          npmMinimalAgeGate: `1d`,
          npmPreapprovedPackages: [`@scoped/release-date`],
        }, async ({run, source}) => {
          await run(`install`);

          await expect(source(`require('@scoped/release-date/package.json')`)).resolves.toMatchObject({
            name: `@scoped/release-date`,
            version: `1.1.2`,
          });
        }),
      );

      test(
        `it should install scoped package when excluded by scoped glob pattern`,
        makeTemporaryEnv({
          dependencies: {[`@scoped/release-date`]: `^1.0.0`},
        }, {
          npmMinimalAgeGate: `1d`,
          npmPreapprovedPackages: [`@scoped/*`],
        }, async ({run, source}) => {
          await run(`install`);

          await expect(source(`require('@scoped/release-date/package.json')`)).resolves.toMatchObject({
            name: `@scoped/release-date`,
            version: `1.1.2`,
          });
        }),
      );

      test(
        `it should not install a version that is higher than the latest tag`,
        makeTemporaryEnv({
          dependencies: {[`@scoped/release-date`]: `latest`},
        }, {
          npmMinimalAgeGate: `1d`,
        }, async ({run, source}) => {
          await run(`install`);

          await expect(source(`require('@scoped/release-date/package.json')`)).resolves.toMatchObject({
            name: `@scoped/release-date`,
            version: `1.1.0`,
          });
        }),
      );
    });
    describe(`up`, () => {
      test(
        `it should update to the latest version allowed by the minimum release age`,
        makeTemporaryEnv({
          dependencies: {[`release-date`]: `^1.0.0`},
        }, {
          npmMinimalAgeGate: `1d`,
        }, async ({run, source}) => {
          await run(`install`);
          await run(`set`, `resolution`, `release-date@npm:^1.0.0`, `npm:1.0.0`);

          const preUpVersion = (await source(`require('release-date/package.json')`)).version;
          if (preUpVersion !== `1.0.0`)
            throw new Error(`Pre-up version is not 1.0.0`);

          await run(`up`, `release-date`);
          await expect(source(`require('release-date/package.json')`)).resolves.toMatchObject({
            name: `release-date`,
            version: `1.1.0`,
          });
        }),
      );
      test(
        `recursive should update to the latest version allowed by the minimum release age`,
        makeTemporaryEnv({
          dependencies: {[`release-date`]: `^1.0.0`},
        }, {
          // we are checking a transitive dependencies version, which the pnp will throw an error for
          // disabling these checks for the purpose of this test
          pnpFallbackMode: `all`,
          pnpMode: `loose`,
          npmMinimalAgeGate: `1d`,
        }, async ({run, source}) => {
          await run(`install`);
          await run(`set`, `resolution`, `release-date@npm:^1.0.0`, `npm:1.0.0`);
          await run(`set`, `resolution`, `release-date-transitive@npm:^1.0.0`, `npm:1.0.0`);

          const preUpVersion = (await source(`require('release-date/package.json')`)).version;
          const preUpVersionTransitive = (await source(`require('release-date/package.json')`)).version;
          if (preUpVersion !== `1.0.0` || preUpVersionTransitive !== `1.0.0`)
            throw new Error(`Pre-up version is not 1.0.0`);

          await run(`up`, `-R`, `*`);
          await expect(source(`require('release-date/package.json')`)).resolves.toMatchObject({
            name: `release-date`,
            version: `1.1.0`,
          });
          await expect(source(`require('release-date-transitive/package.json')`)).resolves.toMatchObject({
            name: `release-date-transitive`,
            version: `1.1.0`,
          });
        }),
      );

      test(
        `it should not update to a version that is higher than the latest tag`,
        makeTemporaryEnv({
          dependencies: {[`@scoped/release-date`]: `^1.0.0`},
        }, {
          npmMinimalAgeGate: `1d`,
        }, async ({run, source}) => {
          await run(`set`, `resolution`, `@scoped/release-date@npm:^1.0.0`, `npm:1.0.0`);

          await run(`up`, `@scoped/release-date`);

          await expect(source(`require('@scoped/release-date/package.json')`)).resolves.toMatchObject({
            name: `@scoped/release-date`,
            version: `1.1.0`,
          });
        }),
      );
    });

    // The global npmMinimalAgeGate is passed through the environment (rather
    // than written in the rc file) because the test environment already sets
    // YARN_NPM_MINIMAL_AGE_GATE, which would take precedence over the file.
    describe(`npmScopes override`, () => {
      test(
        `scope override tightens the gate beyond the global value`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: `1m`,
        }, async ({path, run}) => {
          const registryUrl = await startPackageServer();
          await writeJson(`${path}/.yarnrc.yml`, {
            npmScopes: {
              scoped: {
                npmRegistryServer: registryUrl,
                npmMinimalAgeGate: `1d`,
              },
            },
          });

          await expect(run(`add`, `@scoped/release-date@1.1.1`)).rejects.toThrowError(`No candidates found`);
        }),
      );

      test(
        `scope override loosens the gate below the global value`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: `1d`,
        }, async ({path, run, source}) => {
          const registryUrl = await startPackageServer();
          await writeJson(`${path}/.yarnrc.yml`, {
            npmScopes: {
              scoped: {
                npmRegistryServer: registryUrl,
                npmMinimalAgeGate: 0,
              },
            },
          });

          await run(`add`, `@scoped/release-date@^1.0.0`);

          await expect(source(`require('@scoped/release-date/package.json')`)).resolves.toMatchObject({
            name: `@scoped/release-date`,
            version: `1.1.2`,
          });
        }),
      );

      test(
        `scope override does not affect unscoped packages`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: `1d`,
        }, async ({path, run, source}) => {
          await writeJson(`${path}/.yarnrc.yml`, {
            npmScopes: {
              scoped: {
                npmMinimalAgeGate: 0,
              },
            },
          });

          await run(`add`, `release-date@^1.0.0`);

          await expect(source(`require('release-date/package.json')`)).resolves.toMatchObject({
            name: `release-date`,
            version: `1.1.0`,
          });
        }),
      );

      test(
        `override on a different scope does not leak`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: `1d`,
        }, async ({path, run}) => {
          await writeJson(`${path}/.yarnrc.yml`, {
            npmScopes: {
              other: {
                npmMinimalAgeGate: 0,
              },
            },
          });

          await expect(run(`add`, `@scoped/release-date@1.1.1`)).rejects.toThrowError(`No candidates found`);
        }),
      );

      test(
        `scope override tightens the gate even when the global is disabled`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: 0,
        }, async ({path, run}) => {
          const registryUrl = await startPackageServer();
          await writeJson(`${path}/.yarnrc.yml`, {
            npmScopes: {
              scoped: {
                npmRegistryServer: registryUrl,
                npmMinimalAgeGate: `1d`,
              },
            },
          });

          await expect(run(`add`, `@scoped/release-date@1.1.1`)).rejects.toThrowError(`No candidates found`);
        }),
      );

      test(
        `--no-time-gate bypasses scope override`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: `1d`,
        }, async ({path, run, source}) => {
          const registryUrl = await startPackageServer();
          await writeJson(`${path}/.yarnrc.yml`, {
            npmScopes: {
              scoped: {
                npmRegistryServer: registryUrl,
                npmMinimalAgeGate: `7d`,
              },
            },
          });

          await run(`add`, `--no-time-gate`, `@scoped/release-date@1.1.2`);

          await expect(source(`require('@scoped/release-date/package.json')`)).resolves.toMatchObject({
            name: `@scoped/release-date`,
            version: `1.1.2`,
          });
        }),
      );

      test(
        `it should expose the scope override through yarn config get`,
        makeTemporaryEnv({}, async ({path, run}) => {
          await writeFile(`${path}/.yarnrc.yml`, [
            `npmScopes:`,
            `  scoped:`,
            `    npmMinimalAgeGate: 7d`,
          ].join(`\n`));

          await expect(run(`config`, `get`, `npmScopes.scoped.npmMinimalAgeGate`)).resolves.toMatchObject({
            stdout: `604800s\n`,
          });
        }),
      );

      test(
        `package rules take precedence over the scope override`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: `1d`,
        }, async ({path, run, source}) => {
          await writeJson(`${path}/.yarnrc.yml`, {
            npmScopes: {
              scoped: {
                npmMinimalAgeGate: `7d`,
              },
            },
            packageRules: [{
              packageFilter: `@scoped/*`,
              npmMinimalAgeGate: 0,
            }],
          });

          await run(`add`, `@scoped/release-date@^1.0.0`);

          await expect(source(`require('@scoped/release-date/package.json')`)).resolves.toMatchObject({
            name: `@scoped/release-date`,
            version: `1.1.2`,
          });
        }),
      );

      test(
        `source rules take precedence over the scope override`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: `1d`,
        }, async ({path, run}) => {
          await writeJson(`${path}/.yarnrc.yml`, {
            npmScopes: {
              scoped: {
                npmMinimalAgeGate: 0,
              },
            },
            sourceRules: [{
              ecosystemFilter: `npm`,
              npmMinimalAgeGate: `1d`,
            }],
          });

          await expect(run(`add`, `@scoped/release-date@1.1.1`)).rejects.toThrowError(`No candidates found`);
        }),
      );
    });

    describe(`packageRules override`, () => {
      test(
        `package rule tightens the gate beyond the global value`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: `1m`,
        }, async ({path, run}) => {
          await writeJson(`${path}/.yarnrc.yml`, {
            packageRules: [{
              packageFilter: `@scoped/*`,
              npmMinimalAgeGate: `1d`,
            }],
          });

          await expect(run(`add`, `@scoped/release-date@1.1.1`)).rejects.toThrowError(`No candidates found`);
        }),
      );

      test(
        `package rule loosens the gate below the global value`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: `1d`,
        }, async ({path, run, source}) => {
          await writeJson(`${path}/.yarnrc.yml`, {
            packageRules: [{
              packageFilter: `@scoped/*`,
              npmMinimalAgeGate: 0,
            }],
          });

          await run(`add`, `@scoped/release-date@^1.0.0`);

          await expect(source(`require('@scoped/release-date/package.json')`)).resolves.toMatchObject({
            name: `@scoped/release-date`,
            version: `1.1.2`,
          });
        }),
      );

      test(
        `package rule does not affect packages it doesn't match`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: `1d`,
        }, async ({path, run, source}) => {
          await writeJson(`${path}/.yarnrc.yml`, {
            packageRules: [{
              packageFilter: `@scoped/*`,
              npmMinimalAgeGate: 0,
            }],
          });

          await run(`add`, `release-date@^1.0.0`);

          await expect(source(`require('release-date/package.json')`)).resolves.toMatchObject({
            name: `release-date`,
            version: `1.1.0`,
          });
        }),
      );

      test(
        `package rule on a different scope does not leak`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: `1d`,
        }, async ({path, run}) => {
          await writeJson(`${path}/.yarnrc.yml`, {
            packageRules: [{
              packageFilter: `@other/*`,
              npmMinimalAgeGate: 0,
            }],
          });

          await expect(run(`add`, `@scoped/release-date@1.1.1`)).rejects.toThrowError(`No candidates found`);
        }),
      );

      test(
        `package rule tightens the gate even when the global is disabled`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: 0,
        }, async ({path, run}) => {
          await writeJson(`${path}/.yarnrc.yml`, {
            packageRules: [{
              packageFilter: `@scoped/*`,
              npmMinimalAgeGate: `1d`,
            }],
          });

          await expect(run(`add`, `@scoped/release-date@1.1.1`)).rejects.toThrowError(`No candidates found`);
        }),
      );

      test(
        `--no-time-gate bypasses package rules`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: `1d`,
        }, async ({path, run, source}) => {
          await writeJson(`${path}/.yarnrc.yml`, {
            packageRules: [{
              packageFilter: `@scoped/*`,
              npmMinimalAgeGate: `7d`,
            }],
          });

          await run(`add`, `--no-time-gate`, `@scoped/release-date@1.1.2`);

          await expect(source(`require('@scoped/release-date/package.json')`)).resolves.toMatchObject({
            name: `@scoped/release-date`,
            version: `1.1.2`,
          });
        }),
      );
    });

    // Like in Yarn 4/5, a value without unit is a number of minutes (1440 is
    // one day); release-date-hours@1.1.0 was published two hours ago, so it
    // only passes a gate that would read the value as seconds (24 minutes)
    describe(`values without unit`, () => {
      test(
        `it should read a YAML number from .yarnrc.yml as minutes`,
        makeTemporaryEnv({
          dependencies: {[`release-date-hours`]: `^1.0.0`},
        }, {
          npmMinimalAgeGate: undefined,
        }, async ({path, run, source}) => {
          await writeFile(`${path}/.yarnrc.yml`, `npmMinimalAgeGate: 1440\n`);

          await run(`install`);

          await expect(source(`require('release-date-hours/package.json')`)).resolves.toMatchObject({
            name: `release-date-hours`,
            version: `1.0.0`,
          });
        }),
      );

      test(
        `it should read a numeric string from .yarnrc.yml as minutes`,
        makeTemporaryEnv({
          dependencies: {[`release-date-hours`]: `^1.0.0`},
        }, {
          npmMinimalAgeGate: undefined,
        }, async ({path, run, source}) => {
          await writeFile(`${path}/.yarnrc.yml`, `npmMinimalAgeGate: "1440"\n`);

          await run(`install`);

          await expect(source(`require('release-date-hours/package.json')`)).resolves.toMatchObject({
            name: `release-date-hours`,
            version: `1.0.0`,
          });
        }),
      );

      test(
        `it should read the environment variable as minutes`,
        makeTemporaryEnv({
          dependencies: {[`release-date-hours`]: `^1.0.0`},
        }, {
          npmMinimalAgeGate: `1440`,
        }, async ({run, source}) => {
          await run(`install`);

          await expect(source(`require('release-date-hours/package.json')`)).resolves.toMatchObject({
            name: `release-date-hours`,
            version: `1.0.0`,
          });
        }),
      );

      test(
        `it should still read values with an explicit unit in that unit`,
        makeTemporaryEnv({
          dependencies: {[`release-date-hours`]: `^1.0.0`},
        }, {
          npmMinimalAgeGate: `1440s`,
        }, async ({run, source}) => {
          await run(`install`);

          await expect(source(`require('release-date-hours/package.json')`)).resolves.toMatchObject({
            name: `release-date-hours`,
            version: `1.1.0`,
          });
        }),
      );

      test(
        `it should read a YAML number in packageRules as minutes`,
        makeTemporaryEnv({
          dependencies: {[`release-date-hours`]: `^1.0.0`},
        }, async ({path, run, source}) => {
          await writeFile(`${path}/.yarnrc.yml`, [
            `packageRules:`,
            `  - packageFilter: release-date-hours`,
            `    npmMinimalAgeGate: 1440`,
            ``,
          ].join(`\n`));

          await run(`install`);

          await expect(source(`require('release-date-hours/package.json')`)).resolves.toMatchObject({
            name: `release-date-hours`,
            version: `1.0.0`,
          });
        }),
      );

      test(
        `it should read a YAML number in sourceRules as minutes`,
        makeTemporaryEnv({
          dependencies: {[`release-date-hours`]: `^1.0.0`},
        }, async ({path, run, source}) => {
          await writeFile(`${path}/.yarnrc.yml`, [
            `sourceRules:`,
            `  - ecosystemFilter: npm`,
            `    npmMinimalAgeGate: 1440`,
            ``,
          ].join(`\n`));

          await run(`install`);

          await expect(source(`require('release-date-hours/package.json')`)).resolves.toMatchObject({
            name: `release-date-hours`,
            version: `1.0.0`,
          });
        }),
      );
    });

    describe(`packages with no release time metadata (e.g. GitHub Packages)`, () => {
      test(
        `it should install a package with no release time even if npmMinimalAgeGate is set`,
        makeTemporaryEnv({}, {
          npmMinimalAgeGate: `1d`,
        }, async ({run, source}) => {
          await run(`add`, `no-time-deps`);

          await expect(source(`require('no-time-deps/package.json')`)).resolves.toMatchObject({
            name: `no-time-deps`,
            version: `1.0.0`,
          });
        }),
      );
    });
  });
});
