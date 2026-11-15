import {Filename, ppath, xfs} from '@yarnpkg/fslib';
import {parseSyml}            from '@yarnpkg/parsers';
import {tests}                from 'pkg-tests-core';

const {RequestType, startRegistryRecording} = tests;

export {};

describe(`Features`, () => {
  describe(`Pruned native deps`, () => {
    for (const cpu of [`x64`, `x86`]) {
      it(`should by default only download dependencies for the current system (foo-${cpu})`, makeTemporaryEnv({
        dependencies: {
          [`optional-native`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        const recording = await startRegistryRecording(async () => {
          await run(`install`, {
            env: {
              YARN_CPU_OVERRIDE: cpu,
              YARN_OS_OVERRIDE: `foo`,
              YARN_LIBC_OVERRIDE: `none`,
            },
          });
        });

        const tarballRequests = recording.filter(request => {
          return request.type === RequestType.PackageTarball;
        }).sort((a, b) => {
          const aJson = JSON.stringify(a);
          const bJson = JSON.stringify(b);
          return aJson < bJson ? -1 : aJson > bJson ? 1 : 0;
        });

        expect(tarballRequests).toEqual([{
          type: RequestType.PackageTarball,
          localName: `native-foo-${cpu}`,
          version: `1.0.0`,
        }, {
          type: RequestType.PackageTarball,
          localName: `optional-native`,
          version: `1.0.0`,
        }]);
      }));
    }

    const getTarballRequests = (recording: Array<any>) => recording.filter(request => {
      return request.type === RequestType.PackageTarball;
    }).sort((a, b) => {
      const aJson = JSON.stringify(a);
      const bJson = JSON.stringify(b);
      return aJson < bJson ? -1 : aJson > bJson ? 1 : 0;
    }).map(request => {
      return request.localName;
    });

    // Negated entries follow npm: a value must match none of the negated
    // entries, and one of the regular entries if the list contains any
    for (const [os, cpu, libc, expected] of [
      [`foo`, `x64`, `glibc`, [`native-foo-not-bar`, `native-not-musl`]],
      [`bar`, `x86`, `musl`, [`native-not-foo`]],
      [`baz`, `arm64`, `glibc`, [`native-not-foo`, `native-not-musl`, `native-not-x64-x86`]],
    ] as const) {
      it(`should support negated entries in the os, cpu, and libc fields (${os}-${cpu}-${libc})`, makeTemporaryEnv({
        dependencies: {
          [`optional-native-negated`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        const recording = await startRegistryRecording(async () => {
          await run(`install`, {
            env: {
              YARN_CPU_OVERRIDE: cpu,
              YARN_OS_OVERRIDE: os,
              YARN_LIBC_OVERRIDE: libc,
            },
          });
        });

        expect(getTarballRequests(recording)).toEqual([
          ...expected,
          `optional-native-negated`,
        ]);
      }));
    }

    it(`should support negated entries with supportedArchitectures`, makeTemporaryEnv({
      dependencies: {
        [`optional-native-negated`]: `1.0.0`,
      },
    }, async ({path, run, source}) => {
      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        supportedArchitectures: {
          os: [`foo`, `bar`],
          cpu: [`x64`],
          libc: [`glibc`],
        },
      });

      const recording = await startRegistryRecording(async () => {
        await run(`install`);
      });

      expect(getTarballRequests(recording)).toEqual([
        `native-foo-not-bar`,
        `native-not-foo`,
        `native-not-musl`,
        `optional-native-negated`,
      ]);
    }));

    it(`should support negated entries with the list form of supportedArchitectures, including the current system`, makeTemporaryEnv({
      dependencies: {
        [`optional-native-negated`]: `1.0.0`,
      },
    }, async ({path, run, source}) => {
      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        supportedArchitectures: [{
          os: `current`,
          cpu: `current`,
          libc: `current`,
        }, {
          os: `bar`,
          cpu: `x86`,
          libc: `musl`,
        }],
      });

      const env = {
        YARN_CPU_OVERRIDE: `x64`,
        YARN_OS_OVERRIDE: `foo`,
        YARN_LIBC_OVERRIDE: `glibc`,
      };

      const recording = await startRegistryRecording(async () => {
        await run(`install`, {env});
      });

      expect(getTarballRequests(recording)).toEqual([
        `native-foo-not-bar`,
        `native-not-foo`,
        `native-not-musl`,
        `optional-native-negated`,
      ]);
    }));

    it(`should keep the negated entries in the lockfile`, makeTemporaryEnv({
      dependencies: {
        [`optional-native-negated`]: `1.0.0`,
      },
    }, async ({path, run, source}) => {
      const env = {
        YARN_CPU_OVERRIDE: `x64`,
        YARN_OS_OVERRIDE: `foo`,
        YARN_LIBC_OVERRIDE: `glibc`,
      };

      await run(`install`, {env});

      const file = parseSyml(await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`));

      const dependencies = tests.FEATURE_CHECKS.jsonLockfile
        ? file.entries
        : file;

      expect(dependencies[`native-not-foo@npm:1.0.0`].resolution.requirements).toEqual({os: [`!foo`]});
      expect(dependencies[`native-not-x64-x86@npm:1.0.0`].resolution.requirements).toEqual({cpu: [`!x64`, `!x86`]});
      expect(dependencies[`native-not-musl@npm:1.0.0`].resolution.requirements).toEqual({libc: [`!musl`]});
      expect(dependencies[`native-foo-not-bar@npm:1.0.0`].resolution.requirements).toEqual({os: [`foo`, `!bar`]});

      // The lockfile doesn't depend on the system it was generated on
      const lockfile = await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`);

      await run(`install`, `--immutable`, {
        env: {
          YARN_CPU_OVERRIDE: `x86`,
          YARN_OS_OVERRIDE: `bar`,
          YARN_LIBC_OVERRIDE: `musl`,
        },
      });

      await expect(xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`)).resolves.toEqual(lockfile);

      // Installing again from the lockfile must make the same choices
      // as the resolution did
      await xfs.removePromise(ppath.join(path, `.yarn`));

      const recording = await startRegistryRecording(async () => {
        await run(`install`, `--immutable`, {env});
      });

      expect(getTarballRequests(recording)).toEqual([
        `native-foo-not-bar`,
        `native-not-musl`,
        `optional-native-negated`,
      ]);
    }));

    for (const [os, expected] of [
      [`bar`, true],
      [`foo`, false],
    ] as const) {
      it(`should ${expected ? `install` : `skip`} regular dependencies with negated entries when the system is ${expected ? `included` : `excluded`}`, makeTemporaryEnv({
        dependencies: {
          [`native-not-foo`]: `1.0.0`,
        },
      }, async ({path, run, source}) => {
        await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
          nodeLinker: `node-modules`,
        });

        await run(`install`, {
          env: {
            YARN_OS_OVERRIDE: os,
          },
        });

        expect(xfs.existsSync(ppath.join(path, Filename.nodeModules, `native-not-foo`, Filename.manifest))).toEqual(expected);
      }));
    }

    it(`should resolve all dependencies, regardless of the system`, makeTemporaryEnv({
      dependencies: {
        [`optional-native`]: `1.0.0`,
      },
    }, async ({path, run, source}) => {
      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        supportedArchitectures: {
          cpu: [`foo`],
          os: [`x64`],
          libc: [`glibc`],
        },
      });

      await run(`install`);

      const file = parseSyml(await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`));

      const dependencies = tests.FEATURE_CHECKS.jsonLockfile
        ? file.entries
        : file;

      delete dependencies.__metadata;

      expect(Object.keys(dependencies)).toEqual(expect.arrayContaining([
        `native-bar-x64@npm:1.0.0`,
        `native-foo-x64@npm:1.0.0`,
        `native-foo-x86@npm:1.0.0`,
        `native-libc-glibc@npm:1.0.0`,
        `native-libc-musl@npm:1.0.0`,
        `optional-native@npm:1.0.0`,
        expect.stringMatching(/^root-workspace.*/),
      ]));
      expect(Object.keys(dependencies)).toHaveLength(7);
    }));

    it(`shouldn't fetch packages that it won't need`, makeTemporaryEnv({
      dependencies: {
        [`optional-native`]: `1.0.0`,
      },
    }, async ({path, run, source}) => {
      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        supportedArchitectures: {
          os: [`foo`],
          cpu: [`x64`],
          libc: [`glibc`],
        },
      });

      const recording = await startRegistryRecording(async () => {
        await run(`install`);
      });

      const tarballRequests = recording.filter(request => {
        return request.type === RequestType.PackageTarball;
      }).sort((a, b) => {
        const aJson = JSON.stringify(a);
        const bJson = JSON.stringify(b);
        return aJson < bJson ? -1 : aJson > bJson ? 1 : 0;
      });

      expect(tarballRequests).toEqual([{
        type: RequestType.PackageTarball,
        localName: `native-foo-x64`,
        version: `1.0.0`,
      }, {
        type: RequestType.PackageTarball,
        localName: `native-libc-glibc`,
        version: `1.0.0`,
      }, {
        type: RequestType.PackageTarball,
        localName: `optional-native`,
        version: `1.0.0`,
      }]);
    }));

    it(`should overfetch if requested to do so`, makeTemporaryEnv({
      dependencies: {
        [`optional-native`]: `1.0.0`,
      },
    }, async ({path, run, source}) => {
      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        supportedArchitectures: {
          os: [`foo`],
          cpu: [`x64`, `x86`],
          libc: [`glibc`, `musl`],
        },
      });

      const recording = await startRegistryRecording(async () => {
        await run(`install`);
      });

      const tarballRequests = recording.filter(request => {
        return request.type === RequestType.PackageTarball;
      }).sort((a, b) => {
        const aJson = JSON.stringify(a);
        const bJson = JSON.stringify(b);
        return aJson < bJson ? -1 : aJson > bJson ? 1 : 0;
      });

      expect(tarballRequests).toEqual([{
        type: RequestType.PackageTarball,
        localName: `native-foo-x64`,
        version: `1.0.0`,
      }, {
        type: RequestType.PackageTarball,
        localName: `native-foo-x86`,
        version: `1.0.0`,
      }, {
        type: RequestType.PackageTarball,
        localName: `native-libc-glibc`,
        version: `1.0.0`,
      }, {
        type: RequestType.PackageTarball,
        localName: `native-libc-musl`,
        version: `1.0.0`,
      }, {
        type: RequestType.PackageTarball,
        localName: `optional-native`,
        version: `1.0.0`,
      }]);
    }));

    it(`should only fetch the exact architectures listed when using the list form`, makeTemporaryEnv({
      dependencies: {
        [`optional-native`]: `1.0.0`,
      },
    }, async ({path, run}) => {
      // The matrix form would fetch the cross product of both entries (and
      // thus also fetch native-foo-x86 and native-bar-x64); the list form only
      // fetches packages compatible with one of the entries taken as a whole.
      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        supportedArchitectures: [{
          os: `foo`,
          cpu: `x64`,
          libc: `glibc`,
        }, {
          os: `bar`,
          cpu: `x86`,
          libc: `musl`,
        }],
      });

      const recording = await startRegistryRecording(async () => {
        await run(`install`);
      });

      const tarballRequests = recording.filter(request => {
        return request.type === RequestType.PackageTarball;
      }).sort((a, b) => {
        const aJson = JSON.stringify(a);
        const bJson = JSON.stringify(b);
        return aJson < bJson ? -1 : aJson > bJson ? 1 : 0;
      });

      expect(tarballRequests).toEqual([{
        type: RequestType.PackageTarball,
        localName: `native-foo-x64`,
        version: `1.0.0`,
      }, {
        type: RequestType.PackageTarball,
        localName: `native-libc-glibc`,
        version: `1.0.0`,
      }, {
        type: RequestType.PackageTarball,
        localName: `native-libc-musl`,
        version: `1.0.0`,
      }, {
        type: RequestType.PackageTarball,
        localName: `optional-native`,
        version: `1.0.0`,
      }]);
    }));

    it(`should support the list form with multiple values and nulls inside a single entry`, makeTemporaryEnv({
      dependencies: {
        [`optional-native`]: `1.0.0`,
      },
    }, async ({path, run, source}) => {
      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        supportedArchitectures: [{
          os: `foo`,
          cpu: [`x64`, `x86`],
          libc: null,
        }],
      });

      const recording = await startRegistryRecording(async () => {
        await run(`install`);
      });

      const tarballRequests = recording.filter(request => {
        return request.type === RequestType.PackageTarball;
      }).sort((a, b) => {
        const aJson = JSON.stringify(a);
        const bJson = JSON.stringify(b);
        return aJson < bJson ? -1 : aJson > bJson ? 1 : 0;
      });

      expect(tarballRequests).toEqual([{
        type: RequestType.PackageTarball,
        localName: `native-foo-x64`,
        version: `1.0.0`,
      }, {
        type: RequestType.PackageTarball,
        localName: `native-foo-x86`,
        version: `1.0.0`,
      }, {
        type: RequestType.PackageTarball,
        localName: `native-libc-glibc`,
        version: `1.0.0`,
      }, {
        type: RequestType.PackageTarball,
        localName: `native-libc-musl`,
        version: `1.0.0`,
      }, {
        type: RequestType.PackageTarball,
        localName: `optional-native`,
        version: `1.0.0`,
      }]);
    }));

    it(`should treat a list with a single entry just like the legacy object form`, makeTemporaryEnv({
      dependencies: {
        [`optional-native`]: `1.0.0`,
      },
    }, async ({path, run, source}) => {
      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        supportedArchitectures: [{
          os: [`foo`],
          cpu: [`x64`],
          libc: [`glibc`],
        }],
      });

      const recording = await startRegistryRecording(async () => {
        await run(`install`);
      });

      const tarballRequests = recording.filter(request => {
        return request.type === RequestType.PackageTarball;
      }).sort((a, b) => {
        const aJson = JSON.stringify(a);
        const bJson = JSON.stringify(b);
        return aJson < bJson ? -1 : aJson > bJson ? 1 : 0;
      });

      expect(tarballRequests).toEqual([{
        type: RequestType.PackageTarball,
        localName: `native-foo-x64`,
        version: `1.0.0`,
      }, {
        type: RequestType.PackageTarball,
        localName: `native-libc-glibc`,
        version: `1.0.0`,
      }, {
        type: RequestType.PackageTarball,
        localName: `optional-native`,
        version: `1.0.0`,
      }]);
    }));

    it(`should produce a stable lockfile, regardless of the architecture`, makeTemporaryEnv({
      dependencies: {
        [`optional-native`]: `1.0.0`,
      },
    }, async ({path, run, source}) => {
      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        supportedArchitectures: {
          os: [`foo`],
          cpu: [`x64`],
          libc: [`glibc`],
        },
      });

      await run(`install`);
      const lockfile64 = await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`);

      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        supportedArchitectures: {
          os: [`bar`],
          cpu: [`x86`],
          libc: [`musl`],
        },
      });

      await run(`install`);
      const lockfile86 = await xfs.readFilePromise(ppath.join(path, Filename.lockfile), `utf8`);

      expect(lockfile86).toEqual(lockfile64);
    }));

    it(`should produce a stable PnP hook, regardless of the architecture`, makeTemporaryEnv({
      dependencies: {
        [`optional-native`]: `1.0.0`,
      },
    }, async ({path, run, source}) => {
      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        supportedArchitectures: {
          os: [`foo`],
          cpu: [`x64`],
          libc: [`glibc`],
        },
      });

      await run(`install`);
      const hook64 = await xfs.readFilePromise(ppath.join(path, Filename.pnpCjs), `utf8`);

      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        supportedArchitectures: {
          os: [`bar`],
          cpu: [`x86`],
          libc: [`musl`],
        },
      });

      await run(`install`);
      const hook86 = await xfs.readFilePromise(ppath.join(path, Filename.pnpCjs), `utf8`);

      expect(hook86).toEqual(hook64);
    }));

    it(`shouldn't break when using --check-cache with native packages`, makeTemporaryEnv({
      dependencies: {
        [`optional-native`]: `1.0.0`,
      },
    }, async ({path, run, source}) => {
      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        supportedArchitectures: {
          os: [`foo`],
          cpu: [`x64`],
          libc: [`glibc`],
        },
      });

      await run(`install`);

      const cacheFolder = ppath.join(path, `.yarn/cache`);
      const cacheListing = await xfs.readdirPromise(cacheFolder);
      const nativeFile = cacheListing.find(entry => entry.startsWith(`native-foo-x64-`));

      // Sanity check
      expect(nativeFile).toBeDefined();

      await run(`install`, `--check-cache`);
    }));

    it(`should detect packages being tampered when using --check-cache`, makeTemporaryEnv({
      dependencies: {
        [`optional-native`]: `1.0.0`,
      },
    }, async ({path, run, source}) => {
      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        supportedArchitectures: {
          os: [`foo`],
          cpu: [`x64`],
          libc: [`glibc`],
        },
      });

      await run(`install`);

      const cacheFolder = ppath.join(path, `.yarn/cache`);
      const cacheListing = await xfs.readdirPromise(cacheFolder);
      const nativeFile = cacheListing.find(entry => entry.startsWith(`native-foo-x64-`));

      // Sanity check
      expect(nativeFile).toBeDefined();

      await xfs.appendFilePromise(ppath.join(cacheFolder, nativeFile as Filename), Buffer.from([0]));

      await expect(async () => {
        await run(`install`, `--check-cache`);
      }).rejects.toThrow();
    }));

    it(`should also validate other architectures than the current one if necessary when using --check-cache`, makeTemporaryEnv({
      dependencies: {
        [`optional-native`]: `1.0.0`,
      },
    }, async ({path, run, source}) => {
      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        supportedArchitectures: {
          os: [`foo`],
          cpu: [`x64`, `x86`],
          libc: [`glibc`],
        },
      });

      await run(`install`);

      const cacheFolder = ppath.join(path, `.yarn/cache`);
      const cacheListing = await xfs.readdirPromise(cacheFolder);
      const nativeFile = cacheListing.find(entry => entry.startsWith(`native-foo-x64-`));

      // Sanity check
      expect(nativeFile).toBeDefined();

      await xfs.appendFilePromise(ppath.join(cacheFolder, nativeFile as Filename), Buffer.from([0]));

      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        supportedArchitectures: {
          os: [`foo`],
          cpu: [`x86`],
          libc: [`glibc`],
        },
      });

      await expect(async () => {
        await run(`install`, `--check-cache`);
      }).rejects.toThrow();
    }));

    it(`should only fetch other architectures when using --check-cache if they are already in the cache`, makeTemporaryEnv({
      dependencies: {
        [`optional-native`]: `1.0.0`,
      },
    }, async ({path, run, source}) => {
      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        supportedArchitectures: {
          os: [`foo`],
          cpu: [`x64`, `x86`],
          libc: [`glibc`],
        },
      });

      await run(`install`);

      await xfs.writeJsonPromise(ppath.join(path, Filename.rc), {
        supportedArchitectures: {
          os: [`foo`],
          cpu: [`x64`],
          libc: [`glibc`],
        },
      });

      const recording = await startRegistryRecording(async () => {
        await run(`install`, `--check-cache`);
      });

      const tarballRequests = recording.filter(request => {
        return request.type === RequestType.PackageTarball;
      }).sort((a, b) => {
        const aJson = JSON.stringify(a);
        const bJson = JSON.stringify(b);
        return aJson < bJson ? -1 : aJson > bJson ? 1 : 0;
      });

      expect(tarballRequests).toEqual([{
        type: RequestType.PackageTarball,
        localName: `native-foo-x64`,
        version: `1.0.0`,
      }, {
        type: RequestType.PackageTarball,
        localName: `native-foo-x86`,
        version: `1.0.0`,
      }, {
        type: RequestType.PackageTarball,
        localName: `native-libc-glibc`,
        version: `1.0.0`,
      }, {
        type: RequestType.PackageTarball,
        localName: `optional-native`,
        version: `1.0.0`,
      }]);
    }));
  });
});
