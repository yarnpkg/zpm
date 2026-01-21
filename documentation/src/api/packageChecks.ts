import {formatPackageLink, STANDARD_EXTENSIONS} from '@/utils/helpers';

import {
  usePackageExists,
  usePackageInfo,
  useReleaseInfo,
  useResolution,
} from './package';

export type CheckResult = {
  ok: boolean;
  message?: React.ReactNode;
};

export type Check = {
  id: string;
  defaultEnabled: boolean;
  success: string;
  failure: string;
  useCheck: (params: {
    name: string;
    version: string;
    versionData?: any;
  }) => CheckResult;
};

export const checks: Array<Check> = [
  {
    id: `deprecated`,
    defaultEnabled: true,
    success: `This package isn't deprecated`,
    failure: `This package has been marked deprecated`,
    useCheck: ({versionData}) => {
      if (versionData?.npm?.deprecated)
        return {ok: false};

      return {ok: true};
    },
  },
  {
    id: `cjs`,
    defaultEnabled: true,
    success: `The package has a commonjs entry point`,
    failure: `The package doesn't seem to have a commonjs entry point`,
    useCheck: ({name, version, versionData}) => {
      if (name?.startsWith(`@types/`))
        return {ok: true};

      const resolution = useResolution({
        name,
        version,
        versionData,
      }, {
        mainFields: [`main`],
        conditions: [`default`, `require`, `node`],
      });

      if (!resolution || resolution.endsWith(`.mjs`))
        return {ok: false};

      if (resolution.endsWith(`.js`) && versionData?.npm?.type === `module`)
        return {ok: false};

      return {ok: true};
    },
  },
  {
    id: `esm`,
    defaultEnabled: false,
    success: `The package has an ESM entry point`,
    failure: `The package doesn't seem to have an ESM entry point`,
    useCheck: ({name, version, versionData}) => {
      if (name?.startsWith(`@types/`))
        return {ok: true};

      const resolution = useResolution({
        name,
        version,
        versionData,
      }, {
        mainFields: [`main`],
        conditions: [`default`, `import`, `node`],
      });

      if (!resolution || resolution.endsWith(`.cjs`))
        return {ok: false};

      if (resolution.endsWith(`.js`) && versionData.npm?.type !== `module`)
        return {ok: false};

      return {ok: true};
    },
  },
  {
    id: `postinstall`,
    defaultEnabled: true,
    success: `The package doesn't have postinstall scripts`,
    failure: `The package has postinstall scripts`,
    useCheck: ({versionData}) => {
      for (const name of [`preinstall`, `install`, `postinstall`])
        if (versionData.npm?.scripts?.[name])
          return {ok: false};

      return {ok: true};
    },
  },
  {
    id: `types`,
    defaultEnabled: true,
    success: `The package ships with types`,
    failure: `The package doesn't ship with types`,
    useCheck: ({name, version, versionData}) => {
      const releaseInfo = useReleaseInfo({
        name,
        version,
      });

      const tsExtensions = [
        ...STANDARD_EXTENSIONS.map(ext => ext.replace(`js`, `ts`)),
        ...STANDARD_EXTENSIONS.map(ext => ext.replace(`js`, `tsx`)),
        `.d.ts`,
      ];

      const resolution = useResolution({
        name,
        version,
        versionData,
      }, {
        mainFields: [`types`, `typings`, `main`],
        conditions: [`types`, `default`, `require`, `import`, `node`],
      });

      const dtPackageName = !name?.startsWith(`@types`)
        ? `@types/${name?.replace(/^@([^/]*)\/([^/]*)$/, `$1__$2`)}`
        : null;

      const dtPackage = dtPackageName
        ? usePackageExists(dtPackageName)
        : null;

      const fileNoExt = resolution?.replace(/(\.[mc]?(js|ts)x?|\.d\.ts)$/, ``);
      for (const ext of tsExtensions)
        if (releaseInfo.fileSet.has(`${fileNoExt}${ext}`))
          return {ok: true};

      if (dtPackageName && dtPackage?.data) {
        const result = usePackageInfo(dtPackageName);

        const latest = result[`dist-tags`]?.latest;
        if (!latest)
          return {ok: false};

        const href = formatPackageLink(
          dtPackageName,
          latest,
        );

        return {
          ok: true,
          message: `Types are available via <a href="${href}">${dtPackageName}</a>`,
        };
      }

      return {ok: false};
    },
  },
];
