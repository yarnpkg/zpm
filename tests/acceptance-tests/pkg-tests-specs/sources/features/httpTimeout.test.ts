import {Filename, ppath, xfs}  from '@yarnpkg/fslib';
import http, {RequestListener} from 'http';
import {AddressInfo}           from 'net';

const startServer = async (listener: RequestListener) => {
  const server = http.createServer(listener);
  server.unref();

  await new Promise<void>((resolve, reject) => {
    server.once(`error`, reject);
    server.listen(0, `127.0.0.1`, resolve);
  });

  const {port} = server.address() as AddressInfo;

  return {
    close: () => new Promise<void>((resolve, reject) => {
      server.closeAllConnections();
      server.close(error => error ? reject(error) : resolve());
    }),
    url: `http://127.0.0.1:${port}`,
  };
};

// A registry that never answers: requests only end when Yarn gives up on them
const startStalledRegistry = () => startServer(() => {});

// A registry that answers the metadata request after a short delay; long
// enough to exceed a timeout misread as a few milliseconds, but far below
// the timeouts the tests set
const startSlowRegistry = () => startServer((request, response) => {
  setTimeout(() => {
    if (request.url !== `/no-deps-delayed`) {
      response.writeHead(404);
      response.end();
      return;
    }

    const metadata = JSON.stringify({
      name: `no-deps-delayed`,
      versions: {
        [`1.0.0`]: {
          name: `no-deps-delayed`,
          version: `1.0.0`,
        },
      },
      [`dist-tags`]: {
        latest: `1.0.0`,
      },
    });

    response.writeHead(200, {
      [`Content-Length`]: Buffer.byteLength(metadata),
      [`Content-Type`]: `application/json`,
    });
    response.end(metadata);
  }, 500);
});

// The stalled registry accepts connections, so a timeout is the only way for
// the request to fail
const TIMEOUT_ERROR = /Network error: error sending request for url \(http:\/\/127\.0\.0\.1:\d+\/no-deps\)/;

describe(`Features`, () => {
  describe(`httpTimeout`, () => {
    for (const httpTimeout of [`1s`, `1000`]) {
      test(
        `it should give up on requests after the timeout (${httpTimeout}, environment variable)`,
        makeTemporaryEnv({
          dependencies: {[`no-deps`]: `1.0.0`},
        }, {
          httpTimeout,
          httpRetry: 0,
          unsafeHttpWhitelist: [`127.0.0.1`],
        }, async ({run}) => {
          const server = await startStalledRegistry();

          try {
            await expect(run(`install`, {
              env: {
                YARN_NPM_REGISTRY_SERVER: server.url,
              },
            })).rejects.toThrow(TIMEOUT_ERROR);
          } finally {
            await server.close();
          }
        }),
      );

      test(
        `it should give up on requests after the timeout (${httpTimeout}, .yarnrc.yml)`,
        makeTemporaryEnv({
          dependencies: {[`no-deps`]: `1.0.0`},
        }, {
          httpRetry: 0,
          unsafeHttpWhitelist: [`127.0.0.1`],
        }, async ({path, run}) => {
          const server = await startStalledRegistry();

          try {
            // Unquoted, so that `1000` is a YAML number
            await xfs.writeFilePromise(ppath.join(path, Filename.rc), `httpTimeout: ${httpTimeout}\n`);

            await expect(run(`install`, {
              env: {
                YARN_NPM_REGISTRY_SERVER: server.url,
              },
            })).rejects.toThrow(TIMEOUT_ERROR);
          } finally {
            await server.close();
          }
        }),
      );
    }

    test(
      `it should read the unit form in its unit (environment variable)`,
      makeTemporaryEnv({}, {
        httpTimeout: `1m`,
        httpRetry: 0,
        unsafeHttpWhitelist: [`127.0.0.1`],
      }, async ({run}) => {
        const server = await startSlowRegistry();

        try {
          await expect(run(`npm`, `info`, `no-deps-delayed`, `--json`, {
            env: {
              YARN_NPM_REGISTRY_SERVER: server.url,
            },
          })).resolves.toMatchObject({
            stdout: expect.stringContaining(`no-deps-delayed`),
          });
        } finally {
          await server.close();
        }
      }),
    );

    test(
      `it should read the unit form in its unit (.yarnrc.yml)`,
      makeTemporaryEnv({}, {
        httpRetry: 0,
        unsafeHttpWhitelist: [`127.0.0.1`],
      }, async ({path, run}) => {
        const server = await startSlowRegistry();

        try {
          await xfs.writeFilePromise(ppath.join(path, Filename.rc), `httpTimeout: 30s\n`);

          await expect(run(`npm`, `info`, `no-deps-delayed`, `--json`, {
            env: {
              YARN_NPM_REGISTRY_SERVER: server.url,
            },
          })).resolves.toMatchObject({
            stdout: expect.stringContaining(`no-deps-delayed`),
          });
        } finally {
          await server.close();
        }
      }),
    );
  });
});
