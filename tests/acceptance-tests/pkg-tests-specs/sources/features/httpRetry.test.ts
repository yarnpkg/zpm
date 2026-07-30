import {Filename, ppath, xfs}  from '@yarnpkg/fslib';
import http, {RequestListener} from 'http';
import {AddressInfo}           from 'net';
import {tests}                 from 'pkg-tests-core';

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
      server.close(error => error ? reject(error) : resolve());
    }),
    url: `http://127.0.0.1:${port}`,
  };
};

describe(`Features`, () => {
  describe(`httpRetry`, () => {
    test(
      `it should retry truncated response bodies`,
      makeTemporaryEnv({}, {
        httpRetry: 1,
        unsafeHttpWhitelist: [`127.0.0.1`],
      }, async ({path, run, source}) => {
        const archivePath = await tests.getPackageArchivePath(`no-deps`, `1.0.0`);
        const archive = await xfs.readFilePromise(archivePath);
        let requestCount = 0;

        const server = await startServer((_request, response) => {
          requestCount += 1;

          response.writeHead(200, {
            [`Connection`]: `close`,
            [`Content-Length`]: archive.length,
          });
          response.end(requestCount === 1
            ? archive.subarray(0, Math.floor(archive.length / 2))
            : archive);
        });

        try {
          await xfs.writeJsonPromise(ppath.join(path, Filename.manifest), {
            dependencies: {
              [`no-deps`]: `${server.url}/package.tgz`,
            },
          });

          await run(`install`);

          await expect(source(`require('no-deps')`)).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });
          expect(requestCount).toBe(2);
        } finally {
          await server.close();
        }
      }),
    );

    test(
      `it should preserve truncated authentication responses`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, {
        httpRetry: 1,
        npmAlwaysAuth: true,
        npmAuthToken: `token`,
        unsafeHttpWhitelist: [`127.0.0.1`],
      }, async ({run}) => {
        let requestCount = 0;

        const server = await startServer((_request, response) => {
          requestCount += 1;

          response.writeHead(401, {
            [`Connection`]: `close`,
            [`Content-Length`]: 8,
            [`WWW-Authenticate`]: `OTP`,
          });
          response.end(`cut`);
        });

        try {
          await expect(run(`install`, {
            env: {
              YARN_NPM_REGISTRY_SERVER: server.url,
            },
          })).rejects.toThrow(/Invalid OTP token/);
          expect(requestCount).toBe(1);
        } finally {
          await server.close();
        }
      }),
    );

    test(
      `it should preserve unchecked redirect response bodies`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, {
        unsafeHttpWhitelist: [`127.0.0.1`],
      }, async ({run, source}) => {
        const archivePath = await tests.getPackageArchivePath(`no-deps`, `1.0.0`);
        const archive = await xfs.readFilePromise(archivePath);
        let metadataRequestCount = 0;
        let serverUrl: string;

        const server = await startServer((request, response) => {
          if (request.url === `/no-deps`) {
            metadataRequestCount += 1;

            const metadata = JSON.stringify({
              name: `no-deps`,
              versions: {
                [`1.0.0`]: {
                  name: `no-deps`,
                  version: `1.0.0`,
                  dist: {
                    tarball: `${serverUrl}/no-deps/-/no-deps-1.0.0.tgz`,
                  },
                },
              },
              [`dist-tags`]: {
                latest: `1.0.0`,
              },
            });

            response.writeHead(300, {
              [`Content-Length`]: Buffer.byteLength(metadata),
              [`Content-Type`]: `application/json`,
            });
            response.end(metadata);
          } else {
            response.writeHead(200, {
              [`Content-Length`]: archive.length,
            });
            response.end(archive);
          }
        });
        serverUrl = server.url;

        try {
          await run(`install`, {
            env: {
              YARN_NPM_REGISTRY_SERVER: server.url,
            },
          });

          await expect(source(`require('no-deps')`)).resolves.toMatchObject({
            name: `no-deps`,
            version: `1.0.0`,
          });
          expect(metadataRequestCount).toBe(1);
        } finally {
          await server.close();
        }
      }),
    );
  });
});
