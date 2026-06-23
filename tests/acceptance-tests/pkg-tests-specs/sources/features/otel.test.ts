import {tests} from 'pkg-tests-core';

const {RequestType, startPackageServer, startRegistryRecording} = tests;

const otelEnv = async () => {
  const endpoint = await startPackageServer();

  return {
    OTEL_EXPORTER_OTLP_ENDPOINT: endpoint,
    OTEL_EXPORTER_OTLP_PROTOCOL: `http/json`,
  };
};

type OtelAttribute = {
  key: string;
  value: Record<string, any>;
};

type OtelEvent = {
  name: string;
  attributes?: Array<OtelAttribute>;
};

type OtelSpan = {
  name: string;
  attributes?: Array<OtelAttribute>;
  events?: Array<OtelEvent>;
};

type OtelScopeSpan = {
  spans: Array<OtelSpan>;
};

type OtelResourceSpan = {
  resource: {
    attributes?: Array<OtelAttribute>;
  };
  scopeSpans: Array<OtelScopeSpan>;
};

type OtelExport = {
  resourceSpans: Array<OtelResourceSpan>;
};

const isOtelExport = (body: unknown): body is OtelExport => {
  return typeof body === `object` && body !== null && Array.isArray((body as OtelExport).resourceSpans);
};

const getAttributes = (attributes: Array<OtelAttribute> = []) => {
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

const getOtelPayload = (recording: Array<any>) => {
  const resources = recording.flatMap(request => {
    const body: unknown = request.body;

    if (request.type !== RequestType.OtelTraces || !isOtelExport(body))
      return [];

    return body.resourceSpans.map(resourceSpan => ({
      attributes: getAttributes(resourceSpan.resource.attributes),
    }));
  });

  return {
    resources: [...new Map(resources.map(resource => [JSON.stringify(resource), resource])).values()],
    spans: recording.flatMap(request => {
      const body: unknown = request.body;

      if (request.type !== RequestType.OtelTraces || !isOtelExport(body))
        return [];

      return body.resourceSpans.flatMap(resourceSpan => {
        return resourceSpan.scopeSpans.flatMap(scopeSpan => {
          return scopeSpan.spans.map(span => ({
            name: span.name,
            attributes: getAttributes(span.attributes),
            events: (span.events ?? []).map(event => ({
              name: event.name,
              attributes: getAttributes(event.attributes),
            })),
          }));
        });
      });
    }),
  };
};

const expectToContainExactly = (actual: Array<any>, expected: Array<any>) => {
  expect(actual).toHaveLength(expected.length);

  for (const item of expected) {
    expect(actual).toContainEqual(item);
  }
};

const getSectionSpan = (payload: ReturnType<typeof getOtelPayload>, sectionName: string) => {
  const span = payload.spans.find(span => {
    return span.name === `yarn.report.section` && span.attributes[`section.name`] === sectionName;
  });

  if (typeof span === `undefined`)
    throw new Error(`Expected to find ${sectionName} section span`);

  return span;
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

        expectToContainExactly(coldPayload.resources, [
          expect.objectContaining({
            attributes: expect.objectContaining({
              [`service.name`]: `yarnpkg`,
              [`service.version`]: expect.any(String),
            }),
          }),
        ]);

        expectToContainExactly(coldPayload.spans, [
          expect.objectContaining({
            name: `yarn.report.section`,
            attributes: expect.objectContaining({
              [`section.name`]: `Project validation`,
            }),
            events: [],
          }),
          expect.objectContaining({
            name: `yarn.report.section`,
            attributes: expect.objectContaining({
              [`section.name`]: `Installing packages`,
            }),
          }),
          expect.objectContaining({
            name: `yarn.report.section`,
            attributes: expect.objectContaining({
              [`section.name`]: `Linking the project`,
            }),
            events: [],
          }),
        ]);

        expectToContainExactly(getSectionSpan(coldPayload, `Installing packages`).events, [
          expect.objectContaining({
            name: `yarn.cache.package_download`,
            attributes: expect.objectContaining({
              extension: `.zip`,
              locator: `no-deps@npm:1.0.0`,
            }),
          }),
          expect.objectContaining({
            name: `yarn.resolver.package_add`,
            attributes: expect.objectContaining({
              ident: `no-deps`,
              locator: `no-deps@npm:1.0.0`,
              reference: `npm:1.0.0`,
            }),
          }),
          expect.objectContaining({
            name: `yarn.resolver.package_add`,
            attributes: expect.objectContaining({
              ident: `root-workspace`,
              locator: `root-workspace@workspace:root-workspace`,
              reference: `workspace:root-workspace`,
            }),
          }),
        ]);

        const hotRecording = await startRegistryRecording(async () => {
          await run(`install`, {env});
        });

        const hotPayload = getOtelPayload(hotRecording);

        expectToContainExactly(hotPayload.resources, [
          expect.objectContaining({
            attributes: expect.objectContaining({
              [`service.name`]: `yarnpkg`,
              [`service.version`]: expect.any(String),
            }),
          }),
        ]);

        expectToContainExactly(hotPayload.spans, [
          expect.objectContaining({
            name: `yarn.report.section`,
            attributes: expect.objectContaining({
              [`section.name`]: `Project validation`,
            }),
            events: [],
          }),
          expect.objectContaining({
            name: `yarn.report.section`,
            attributes: expect.objectContaining({
              [`section.name`]: `Installing packages`,
            }),
          }),
          expect.objectContaining({
            name: `yarn.report.section`,
            attributes: expect.objectContaining({
              [`section.name`]: `Linking the project`,
            }),
            events: [],
          }),
        ]);

        expectToContainExactly(getSectionSpan(hotPayload, `Installing packages`).events, [
          expect.objectContaining({
            name: `yarn.resolver.package_add`,
            attributes: expect.objectContaining({
              ident: `no-deps`,
              locator: `no-deps@npm:1.0.0`,
              reference: `npm:1.0.0`,
            }),
          }),
          expect.objectContaining({
            name: `yarn.resolver.package_add`,
            attributes: expect.objectContaining({
              ident: `root-workspace`,
              locator: `root-workspace@workspace:root-workspace`,
              reference: `workspace:root-workspace`,
            }),
          }),
        ]);
      }),
    );
  });
});
