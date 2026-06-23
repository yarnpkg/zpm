import {tests} from 'pkg-tests-core';

const {RequestType, startPackageServer, startRegistryRecording} = tests;

const otelEnv = async () => {
  const endpoint = await startPackageServer();

  return {
    OTEL_EXPORTER_OTLP_ENDPOINT: endpoint,
    OTEL_EXPORTER_OTLP_PROTOCOL: `http/json`,
  };
};

const getAttributes = (attributes: Array<any> = []) => {
  return Object.fromEntries(attributes.map(attribute => {
    const value = attribute.value;

    if (`stringValue` in value)
      return [attribute.key, value.stringValue];

    if (`intValue` in value)
      return [attribute.key, Number(value.intValue)];

    if (`boolValue` in value)
      return [attribute.key, value.boolValue];

    if (`doubleValue` in value)
      return [attribute.key, value.doubleValue];

    return [attribute.key, value];
  }));
};

const compareTimestamps = (a: string, b: string) => {
  return Number(BigInt(a) - BigInt(b));
};

const getOtelPayload = (recording: Array<any>) => {
  return {
    spans: recording.flatMap(request => {
      if (request.type !== RequestType.OtelTraces || typeof request.body !== `object` || request.body === null)
        return [];

      return request.body.resourceSpans.flatMap(resourceSpan => {
        return resourceSpan.scopeSpans.flatMap(scopeSpan => {
          return scopeSpan.spans.map(span => ({
            name: span.name,
            startTimeUnixNano: span.startTimeUnixNano,
            attributes: getAttributes(span.attributes),
            events: (span.events ?? []).map(event => ({
              name: event.name,
              timeUnixNano: event.timeUnixNano,
              attributes: getAttributes(event.attributes),
            })).sort((a, b) => {
              return compareTimestamps(a.timeUnixNano, b.timeUnixNano);
            }),
          }));
        });
      });
    }).sort((a, b) => {
      return compareTimestamps(a.startTimeUnixNano, b.startTimeUnixNano);
    }),
  };
};

describe(`Features`, () => {
  describe(`OpenTelemetry`, () => {
    test(
      `it should export install spans and package events`,
      makeTemporaryEnv({
        dependencies: {
          [`no-deps`]: `1.0.0`,
        },
      }, {
        enableGlobalCache: false,
      }, async ({run}) => {
        const env = await otelEnv();

        const coldRecording = await startRegistryRecording(async () => {
          await run(`install`, {env});
        });

        const coldPayload = getOtelPayload(coldRecording);

        expect(coldPayload).toEqual({
          spans: [
            expect.objectContaining({
              events: [],
              name: `yarn.report.section`,
              attributes: expect.objectContaining({
                [`section.name`]: `Project validation`,
              }),
            }),
            expect.objectContaining({
              events: [
                expect.objectContaining({
                  name: `package downloaded`,
                  attributes: expect.objectContaining({
                    extension: `.zip`,
                    locator: `no-deps@npm:1.0.0`,
                  }),
                }),
              ],
              name: `yarn.report.section`,
              attributes: expect.objectContaining({
                [`section.name`]: `Installing packages`,
              }),
            }),
            expect.objectContaining({
              events: [
                expect.objectContaining({
                  name: `package added to project`,
                }),
              ],
              name: `yarn.resolver.package`,
              attributes: expect.objectContaining({
                ident: `no-deps`,
                locator: `no-deps@npm:1.0.0`,
                reference: `npm:1.0.0`,
              }),
            }),
            expect.objectContaining({
              events: [
                expect.objectContaining({
                  name: `package added to project`,
                }),
              ],
              name: `yarn.resolver.package`,
              attributes: expect.objectContaining({
                ident: `root-workspace`,
                locator: `root-workspace@workspace:root-workspace`,
                reference: `workspace:root-workspace`,
              }),
            }),
            expect.objectContaining({
              events: [],
              name: `yarn.report.section`,
              attributes: expect.objectContaining({
                [`section.name`]: `Linking the project`,
              }),
            }),
          ],
        });

        const hotRecording = await startRegistryRecording(async () => {
          await run(`install`, {env});
        });

        const hotPayload = getOtelPayload(hotRecording);

        expect(hotPayload).toEqual({
          spans: [
            expect.objectContaining({
              events: [],
              name: `yarn.report.section`,
              attributes: expect.objectContaining({
                [`section.name`]: `Project validation`,
              }),
            }),
            expect.objectContaining({
              events: [],
              name: `yarn.report.section`,
              attributes: expect.objectContaining({
                [`section.name`]: `Installing packages`,
              }),
            }),
            expect.objectContaining({
              events: [
                expect.objectContaining({
                  name: `package added to project`,
                }),
              ],
              name: `yarn.resolver.package`,
              attributes: expect.objectContaining({
                ident: `root-workspace`,
                locator: `root-workspace@workspace:root-workspace`,
                reference: `workspace:root-workspace`,
              }),
            }),
            expect.objectContaining({
              events: [
                expect.objectContaining({
                  name: `package added to project`,
                }),
              ],
              name: `yarn.resolver.package`,
              attributes: expect.objectContaining({
                ident: `no-deps`,
                locator: `no-deps@npm:1.0.0`,
                reference: `npm:1.0.0`,
              }),
            }),
            expect.objectContaining({
              events: [],
              name: `yarn.report.section`,
              attributes: expect.objectContaining({
                [`section.name`]: `Linking the project`,
              }),
            }),
          ],
        });
      }),
    );
  });
});
