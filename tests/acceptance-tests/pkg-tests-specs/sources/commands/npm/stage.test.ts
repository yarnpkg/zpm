import {ppath, xfs} from '@yarnpkg/fslib';

const {
  misc,
  tests: {RequestType, startPackageServer, startRegistryRecording, validLogins},
} = require(`pkg-tests-core`);

function extractStageId(jsonStdout: string): string {
  const jsonObjects = misc.parseJsonStream(jsonStdout);
  const result = jsonObjects.find((obj: any) => obj?.stageId);
  if (!result)
    throw new Error(`Could not find stageId in JSON output:\n${jsonStdout}`);

  return result.stageId;
}

describe(`Commands`, () => {
  describe(`npm stage`, () => {
    describe(`workspace registries`, () => {
      for (const registrySource of [`manifest`, `scope`, `package`]) {
        test(
          `it should use the active workspace's ${registrySource} registry and credentials throughout the workflow`,
          makeTemporaryEnv({
            private: true,
            workspaces: [`packages/*`],
          }, async ({path, run}) => {
            const registry = await startPackageServer();
            const cwd = ppath.join(path, `packages/pkg`);
            const manifest = {
              name: `@scope/registry-${registrySource}`,
              version: `1.0.0`,
              ...(registrySource === `manifest` && {
                publishConfig: {registry: `${registry}/registry/manifest/`},
              }),
            };

            await xfs.mkdirpPromise(cwd);
            await xfs.writeJsonPromise(ppath.join(cwd, `package.json`), manifest);
            await xfs.writeJsonPromise(ppath.join(path, `.yarnrc.yml`), {
              npmPublishRegistry: `${registry}/registry/default`,
              npmScopes: {
                scope: {
                  npmPublishRegistry: `${registry}/registry/scope`,
                  ...(registrySource === `scope` && {npmAuthToken: validLogins.fooUser.npmAuthToken}),
                },
              },
              ...(registrySource !== `scope` && {
                packageRules: [{
                  packageFilter: `@scope/*`,
                  npmPublishRegistry: `${registry}/registry/package`,
                  ...(registrySource === `package` && {npmAuthToken: validLogins.fooUser.npmAuthToken}),
                }],
              }),
              ...(registrySource === `manifest` && {
                sourceRules: [{
                  registryFilter: `${registry}/registry/manifest`,
                  npmAuthToken: validLogins.fooUser.npmAuthToken,
                }],
              }),
            });

            await run(`install`);

            // Each selected registry must override the unusable default credentials.
            const options = {cwd, env: {YARN_NPM_AUTH_TOKEN: `invalid-token`}};
            const requests = await startRegistryRecording(async () => {
              const {stdout: firstPublish} = await run(`npm`, `publish`, `--staged`, `--json`, options);
              const firstId = extractStageId(firstPublish);

              manifest.version = `2.0.0`;
              await xfs.writeJsonPromise(ppath.join(cwd, `package.json`), manifest);
              const {stdout: secondPublish} = await run(`npm`, `publish`, `--staged`, `--json`, options);
              const secondId = extractStageId(secondPublish);

              const {stdout: listOut} = await run(`npm`, `stage`, `list`, manifest.name, `--json`, options);
              expect(misc.parseJsonStream(listOut).map((item: any) => item.children.ID)).toEqual([firstId, secondId]);

              await run(`npm`, `stage`, `approve`, firstId, options);
              await run(`npm`, `stage`, `reject`, secondId, options);
            });

            expect(requests).toEqual([
              expect.objectContaining({type: RequestType.StagePublish, registry: registrySource}),
              expect.objectContaining({type: RequestType.StagePublish, registry: registrySource}),
              expect.objectContaining({type: RequestType.StageList, registry: registrySource}),
              expect.objectContaining({type: RequestType.StageApprove, registry: registrySource}),
              expect.objectContaining({type: RequestType.StageReject, registry: registrySource}),
            ]);
          }),
        );
      }
    });

    describe(`publish --staged`, () => {
      test(
        `it should stage a package for later approval`,
        makeTemporaryEnv({
          name: `staged-pkg`,
          version: `1.0.0`,
        }, async ({run}) => {
          await run(`install`);

          const {stdout} = await run(`npm`, `publish`, `--staged`, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          });

          expect(stdout).toContain(`Staging to`);
          expect(stdout).toContain(`staged for approval`);
        }),
      );

      test(
        `it should not require OTP for staged publishing`,
        makeTemporaryEnv({
          name: `staged-otp-pkg`,
          version: `1.0.0`,
        }, async ({run}) => {
          await run(`install`);

          const {stdout} = await run(`npm`, `publish`, `--staged`, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.otpUser.npmAuthToken,
            },
          });

          expect(stdout).toContain(`staged for approval`);
        }),
      );

      test(
        `it should support --dry-run with --staged`,
        makeTemporaryEnv({
          name: `staged-dry-run`,
          version: `1.0.0`,
        }, async ({run}) => {
          await run(`install`);

          const requests = await startRegistryRecording(async () => {
            const {stdout} = await run(`npm`, `publish`, `--staged`, `--dry-run`, `--tolerate-republish`);
            expect(stdout).toContain(`Staging to`);
            expect(stdout).toContain(`dry run`);
          });

          expect(requests).not.toEqual(expect.arrayContaining([
            expect.objectContaining({type: RequestType.StagePublish}),
          ]));
        }),
      );

      test(
        `it should support --json with --staged`,
        makeTemporaryEnv({
          name: `staged-json`,
          version: `1.0.0`,
        }, async ({run}) => {
          await run(`install`);

          const {stdout} = await run(`npm`, `publish`, `--staged`, `--json`, `--dry-run`, `--tolerate-republish`);
          const jsonObjects = misc.parseJsonStream(stdout);
          const result = jsonObjects.find((obj: any) => obj?.name && obj?.version);

          expect(result).toBeDefined();
          expect(result).toHaveProperty(`staged`, true);
          expect(result).toHaveProperty(`published`, false);
          expect(result).not.toHaveProperty(`stageId`);
        }),
      );

      test(
        `it should fail without authentication`,
        makeTemporaryEnv({
          name: `staged-no-auth`,
          version: `1.0.0`,
        }, async ({run}) => {
          await run(`install`);

          await expect(run(`npm`, `publish`, `--staged`)).rejects.toThrow();
        }),
      );
    });

    describe(`list`, () => {
      test(
        `it should preserve scoped package names and tags in JSON and use the publish registry`,
        makeTemporaryEnv({
          name: `@scope/staged-json`,
          version: `1.2.3`,
        }, async ({run}) => {
          await run(`install`);

          const registry = `${await startPackageServer()}/registry/publish`;
          const env = {
            YARN_NPM_AUTH_TOKEN: validLogins.otpUser.npmAuthToken,
            YARN_NPM_PUBLISH_REGISTRY: registry,
          };

          const requests = await startRegistryRecording(async () => {
            const {stdout: publishOut} = await run(`npm`, `publish`, `--staged`, `--json`, `--tag`, `next`, {env});
            const stageId = extractStageId(publishOut);
            const result = misc.parseJsonStream(publishOut).find((obj: any) => obj?.stageId);

            expect(result).toMatchObject({staged: true, published: false, dryRun: false, tag: `next`});

            const {stdout} = await run(`npm`, `stage`, `list`, `@scope/staged-json`, `--json`, {env});
            expect(misc.parseJsonStream(stdout)).toEqual([{
              value: {
                descriptor: `@scope/staged-json@next`,
                locator: `@scope/staged-json@npm:1.2.3`,
              },
              children: {
                ID: stageId,
                Staged: expect.any(String),
              },
            }]);

            await run(`npm`, `stage`, `approve`, stageId, `--otp`, validLogins.otpUser.npmOtpToken, {env});
          });

          expect(requests).toEqual(expect.arrayContaining([
            expect.objectContaining({type: RequestType.StagePublish, registry: `publish`, scope: `@scope`, localName: `staged-json`}),
            expect.objectContaining({type: RequestType.StageList, registry: `publish`, packageFilter: `@scope/staged-json`}),
            expect.objectContaining({type: RequestType.StageApprove, registry: `publish`}),
          ]));
        }),
      );

      test(
        `it should fetch every page of staged packages`,
        makeTemporaryEnv({}, async ({run}) => {
          await run(`install`);

          const registry = await startPackageServer();
          const token = validLogins.fooUser.npmAuthToken;

          await Promise.all(Array.from({length: 101}, async (_, index) => {
            const version = `1.0.${index}`;
            const response = await fetch(`${registry}/-/stage/package/paginated-stage`, {
              method: `POST`,
              headers: {authorization: `Bearer ${token}`, [`content-type`]: `application/json`},
              body: JSON.stringify({versions: {[version]: {}}, [`dist-tags`]: {latest: version}}),
            });
            expect(response.status).toBe(201);
            await response.text();
          }));

          const requests = await startRegistryRecording(async () => {
            const {stdout} = await run(`npm`, `stage`, `list`, `paginated-stage`, `--json`, {
              env: {YARN_NPM_AUTH_TOKEN: token},
            });
            const items = misc.parseJsonStream(stdout);
            expect(items).toHaveLength(101);
            expect(new Set(items.map((item: any) => item.children.ID)).size).toBe(101);
          });

          expect(requests).toEqual([
            {type: RequestType.StageList, packageFilter: `paginated-stage`, page: 0, perPage: 100},
            {type: RequestType.StageList, packageFilter: `paginated-stage`, page: 1, perPage: 100},
          ]);
        }),
      );

      test(
        `it should list an empty list when no packages are staged`,
        makeTemporaryEnv({}, async ({run}) => {
          await run(`install`);

          const {stdout} = await run(`npm`, `stage`, `list`, `no-such-staged-pkg`, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          });

          expect(stdout).toContain(`No staged versions found`);
        }),
      );

      test(
        `it should list staged packages after staging one`,
        makeTemporaryEnv({
          name: `list-after-stage`,
          version: `2.0.0`,
        }, async ({run}) => {
          await run(`install`);

          await run(`npm`, `publish`, `--staged`, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          });

          const {stdout} = await run(`npm`, `stage`, `list`, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          });

          expect(stdout).toContain(`list-after-stage`);
          expect(stdout).toContain(`2.0.0`);
        }),
      );

      test(
        `it should filter by package name`,
        makeTemporaryEnv({
          name: `filter-test-pkg`,
          version: `1.0.0`,
        }, async ({run}) => {
          await run(`install`);

          await run(`npm`, `publish`, `--staged`, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          });

          const {stdout: matchOutput} = await run(`npm`, `stage`, `list`, `filter-test-pkg`, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          });

          expect(matchOutput).toContain(`filter-test-pkg`);

          const {stdout: noMatchOutput} = await run(`npm`, `stage`, `list`, `nonexistent-pkg`, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          });

          expect(noMatchOutput).toContain(`No staged versions found`);
        }),
      );

      test(
        `it should fail without authentication`,
        makeTemporaryEnv({}, async ({run}) => {
          await run(`install`);

          await expect(run(`npm`, `stage`, `list`)).rejects.toThrow();
        }),
      );
    });

    describe(`approve`, () => {
      for (const withOtp of [false, true]) {
        test(
          `it should approve multiple versions ${withOtp ? `with` : `without`} OTP`,
          makeTemporaryEnv({
            name: `approve-multiple-${withOtp}`,
            version: `1.0.0`,
          }, async ({path, run}) => {
            await run(`install`);

            const login = withOtp ? validLogins.otpUser : validLogins.fooUser;
            const env = {YARN_NPM_AUTH_TOKEN: login.npmAuthToken};
            const manifestPath = ppath.join(path, `package.json`);
            const manifest = await xfs.readJsonPromise(manifestPath);
            const stageIds = [];

            for (const version of [`1.0.0`, `2.0.0`, `3.0.0`]) {
              await xfs.writeJsonPromise(manifestPath, {...manifest, version});
              const {stdout} = await run(`npm`, `publish`, `--staged`, `--json`, {env});
              stageIds.push(extractStageId(stdout));
            }

            const otpArgs = withOtp ? [`--otp`, login.npmOtpToken] : [];
            const {stdout} = await run(`npm`, `stage`, `approve`, ...stageIds, ...otpArgs, {env});
            for (const id of stageIds)
              expect(stdout).toContain(`Staged package ${id} approved and published successfully.`);

            const {stdout: listOut} = await run(`npm`, `stage`, `list`, manifest.name, `--json`, {env});
            expect(listOut).toBe(``);
          }),
        );
      }

      test(
        `it should validate every stage ID before approving any versions`,
        makeTemporaryEnv({
          name: `approve-validate-all`,
          version: `1.0.0`,
        }, async ({run}) => {
          await run(`install`);

          const env = {YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken};
          const {stdout} = await run(`npm`, `publish`, `--staged`, `--json`, {env});
          const stageId = extractStageId(stdout);

          const requests = await startRegistryRecording(async () => {
            await expect(run(`npm`, `stage`, `approve`, stageId, `not-a-uuid`, {env})).rejects.toThrow(/Invalid npm stage ID/);
            await expect(run(`npm`, `stage`, `approve`, {env})).rejects.toThrow();
          });

          expect(requests).toEqual([]);
          const {stdout: listOut} = await run(`npm`, `stage`, `list`, `approve-validate-all`, `--json`, {env});
          expect(misc.parseJsonStream(listOut)).toEqual([expect.objectContaining({
            children: expect.objectContaining({ID: stageId}),
          })]);
        }),
      );

      test(
        `it should stop on a failed approval and retain earlier successful approvals`,
        makeTemporaryEnv({
          name: `approve-partial-failure`,
          version: `1.0.0`,
        }, async ({path, run}) => {
          await run(`install`);

          const env = {YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken};
          const {stdout: firstPublish} = await run(`npm`, `publish`, `--staged`, `--json`, {env});
          const firstId = extractStageId(firstPublish);

          const manifestPath = ppath.join(path, `package.json`);
          const manifest = await xfs.readJsonPromise(manifestPath);
          await xfs.writeJsonPromise(manifestPath, {...manifest, version: `2.0.0`});
          const {stdout: secondPublish} = await run(`npm`, `publish`, `--staged`, `--json`, {env});
          const secondId = extractStageId(secondPublish);
          const missingId = `1de6f3db-2ed9-4d72-b3dd-8f0e2b474a2f`;

          const requests = await startRegistryRecording(async () => {
            await expect(run(`npm`, `stage`, `approve`, firstId, missingId, secondId, {env})).rejects.toThrow();
          });

          expect(requests).toEqual([
            {type: RequestType.StageApprove, stageId: firstId},
            {type: RequestType.StageApprove, stageId: missingId},
          ]);

          const {stdout: listOut} = await run(`npm`, `stage`, `list`, manifest.name, `--json`, {env});
          expect(misc.parseJsonStream(listOut)).toEqual([expect.objectContaining({
            children: expect.objectContaining({ID: secondId}),
          })]);
        }),
      );

      test(
        `it should approve a staged package`,
        makeTemporaryEnv({
          name: `approve-test`,
          version: `1.0.0`,
        }, async ({run}) => {
          await run(`install`);

          const {stdout: publishOut} = await run(`npm`, `publish`, `--staged`, `--json`, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          });

          const stageId = extractStageId(publishOut);

          const {stdout} = await run(`npm`, `stage`, `approve`, stageId, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          });

          expect(stdout).toContain(`approved and published successfully`);
        }),
      );

      test(
        `it should approve with OTP`,
        makeTemporaryEnv({
          name: `approve-otp-test`,
          version: `1.0.0`,
        }, async ({run}) => {
          await run(`install`);

          const {stdout: publishOut} = await run(`npm`, `publish`, `--staged`, `--json`, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          });

          const stageId = extractStageId(publishOut);

          const {stdout} = await run(`npm`, `stage`, `approve`, stageId, `--otp`, validLogins.otpUser.npmOtpToken, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.otpUser.npmAuthToken,
            },
          });

          expect(stdout).toContain(`approved and published successfully`);
        }),
      );

      test(
        `it should fail with invalid stage ID format`,
        makeTemporaryEnv({}, async ({run}) => {
          await run(`install`);

          await expect(run(`npm`, `stage`, `approve`, `not-a-uuid`, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          })).rejects.toThrow(/Invalid npm stage ID/);
        }),
      );

      test(
        `it should fail with non-existent stage ID`,
        makeTemporaryEnv({}, async ({run}) => {
          await run(`install`);

          await expect(run(`npm`, `stage`, `approve`, `1de6f3db-2ed9-4d72-b3dd-8f0e2b474a2f`, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          })).rejects.toThrow();
        }),
      );

      test(
        `it should fail without authentication`,
        makeTemporaryEnv({}, async ({run}) => {
          await run(`install`);

          await expect(run(`npm`, `stage`, `approve`, `1de6f3db-2ed9-4d72-b3dd-8f0e2b474a2f`)).rejects.toThrow();
        }),
      );
    });

    describe(`reject`, () => {
      test(
        `it should reject with OTP and use the publish registry`,
        makeTemporaryEnv({
          name: `reject-otp-test`,
          version: `1.0.0`,
        }, async ({run}) => {
          await run(`install`);

          const env = {
            YARN_NPM_AUTH_TOKEN: validLogins.otpUser.npmAuthToken,
            YARN_NPM_PUBLISH_REGISTRY: `${await startPackageServer()}/registry/publish`,
          };

          const {stdout: publishOut} = await run(`npm`, `publish`, `--staged`, `--json`, {env});
          const stageId = extractStageId(publishOut);

          await expect(run(`npm`, `stage`, `reject`, stageId, `--otp`, `invalid_otp`, {env})).rejects.toThrow(/Invalid OTP token/);

          const requests = await startRegistryRecording(async () => {
            const {stdout} = await run(`npm`, `stage`, `reject`, stageId, `--otp`, validLogins.otpUser.npmOtpToken, {env});
            expect(stdout).toContain(`has been rejected`);
          });

          expect(requests).toContainEqual({type: RequestType.StageReject, registry: `publish`, stageId});
        }),
      );

      test(
        `it should reject a staged package`,
        makeTemporaryEnv({
          name: `reject-test`,
          version: `1.0.0`,
        }, async ({run}) => {
          await run(`install`);

          const {stdout: publishOut} = await run(`npm`, `publish`, `--staged`, `--json`, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          });

          const stageId = extractStageId(publishOut);

          const {stdout} = await run(`npm`, `stage`, `reject`, stageId, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          });

          expect(stdout).toContain(`has been rejected`);
        }),
      );

      test(
        `it should fail with invalid stage ID format`,
        makeTemporaryEnv({}, async ({run}) => {
          await run(`install`);

          await expect(run(`npm`, `stage`, `reject`, `not-a-uuid`, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          })).rejects.toThrow(/Invalid npm stage ID/);
        }),
      );

      test(
        `it should fail with non-existent stage ID`,
        makeTemporaryEnv({}, async ({run}) => {
          await run(`install`);

          await expect(run(`npm`, `stage`, `reject`, `1de6f3db-2ed9-4d72-b3dd-8f0e2b474a2f`, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          })).rejects.toThrow();
        }),
      );
    });

    describe(`full workflow`, () => {
      test(
        `it should support a stage -> list -> approve workflow`,
        makeTemporaryEnv({
          name: `workflow-approve`,
          version: `1.0.0`,
        }, async ({run}) => {
          await run(`install`);

          const {stdout: publishOut} = await run(`npm`, `publish`, `--staged`, `--json`, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          });

          const stageId = extractStageId(publishOut);

          // List and verify it appears
          const {stdout: listOut} = await run(`npm`, `stage`, `list`, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          });
          expect(listOut).toContain(`workflow-approve`);
          expect(listOut).toContain(stageId);

          // Approve
          await run(`npm`, `stage`, `approve`, stageId, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          });

          // Verify it's gone from the list
          const {stdout: listAfter} = await run(`npm`, `stage`, `list`, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          });
          expect(listAfter).not.toContain(stageId);
        }),
      );

      test(
        `it should support a stage -> list -> reject workflow`,
        makeTemporaryEnv({
          name: `workflow-reject`,
          version: `1.0.0`,
        }, async ({run}) => {
          await run(`install`);

          const {stdout: publishOut} = await run(`npm`, `publish`, `--staged`, `--json`, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          });

          const stageId = extractStageId(publishOut);

          // List and verify it appears
          const {stdout: listOut} = await run(`npm`, `stage`, `list`, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          });
          expect(listOut).toContain(`workflow-reject`);

          // Reject
          await run(`npm`, `stage`, `reject`, stageId, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          });

          // Verify it's gone from the list
          const {stdout: listAfter} = await run(`npm`, `stage`, `list`, {
            env: {
              YARN_NPM_AUTH_TOKEN: validLogins.fooUser.npmAuthToken,
            },
          });
          expect(listAfter).not.toContain(stageId);
        }),
      );
    });
  });
});
