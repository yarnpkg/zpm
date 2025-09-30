import hotT1              from '/src/assets/img/ico-hot-t1.svg?url';
import hotT2              from '/src/assets/img/ico-hot-t2.svg?url';
import hotT3              from '/src/assets/img/ico-hot-t3.svg?url';
import hotT4              from '/src/assets/img/ico-hot-t4.svg?url';
import relativeTime       from 'dayjs/plugin/relativeTime';
import dayjs              from 'dayjs';
import resolve            from 'resolve';
import type {ReleaseInfo} from 'src/types/package';

export const STANDARD_EXTENSIONS = [`.js`, `.cjs`, `.mjs`];

export function normalizeRepoUrl(url: string): string {
  if (!url) return ``;

  try {
    let cleanedUrl = url.trim();

    if (cleanedUrl.startsWith(`git+`))
      cleanedUrl = cleanedUrl.slice(4);


    if (cleanedUrl.startsWith(`git://`))
      cleanedUrl = cleanedUrl.replace(`git://`, `https://`);


    cleanedUrl = cleanedUrl.replace(/^ssh:\/\/git@/, `https://`);

    if (cleanedUrl.startsWith(`git@`)) {
      const match = cleanedUrl.match(/^git@([^:]+):(.+)$/);
      if (match) {
        cleanedUrl = `https://${match[1]}/${match[2]}`;
      }
    }

    if (cleanedUrl.endsWith(`.git`))
      cleanedUrl = cleanedUrl.slice(0, -4);


    return cleanedUrl;
  } catch {
    return url;
  }
}

function getResolutionFunction(
  releaseInfo: ReleaseInfo,
  {extensions = STANDARD_EXTENSIONS}: {extensions?: Array<string>} = {},
) {
  return (qualifier: string) =>
    resolve.sync(qualifier, {
      basedir: `/`,
      includeCoreModules: true,
      paths: [],
      extensions,
      isFile: (path: string) =>
        releaseInfo.jsdelivr.files.some(file => file.name === path),
      isDirectory: (path: string) =>
        releaseInfo.jsdelivr.files.some(file =>
          file.name.startsWith(`${path}/`),
        ),
      realpathSync: (path: string) => path,
      readPackageSync: (_: any, path: string) => {
        if (path === `/package.json`) {
          return releaseInfo.npm as unknown as Record<string, unknown>;
        } else {
          throw new Error(`Failed`);
        }
      },
    });
}

export function resolveQualifier(releaseInfo: ReleaseInfo, qualifier: string) {
  const resolvedQualifier = new URL(qualifier, `https://example.com/`).pathname;
  const resolutionFunction = getResolutionFunction(releaseInfo);

  try {
    return resolutionFunction(resolvedQualifier);
  } catch {
    return null;
  }
}

dayjs.extend(relativeTime);

export function formatDate(date: Date): string {
  const now = dayjs();
  const input = dayjs(date);
  const diffInSeconds = now.diff(input, `second`);

  const units = [
    {label: `year`, seconds: 31536000},
    {label: `month`, seconds: 2592000},
    {label: `week`, seconds: 604800},
    {label: `day`, seconds: 86400},
    {label: `hour`, seconds: 3600},
    {label: `minute`, seconds: 60},
    {label: `second`, seconds: 1},
  ];

  for (const {label, seconds} of units) {
    const value = Math.floor(diffInSeconds / seconds);
    if (value >= 1) {
      return `${value} ${label}${value > 1 ? `s` : ``} ago`;
    }
  }

  return `just now`;
}

export function formatPackageLink(
  name: string,
  version: string,
  file?: string,
) {
  const encodedName = encodeURIComponent(name);
  const encodedVersion = encodeURIComponent(version);
  const encodedFile = file && encodeURIComponent(file);

  let path = `/package/${encodedName}/${encodedVersion}`;
  if (encodedFile)
    path = `${path}/${encodedFile}`;

  return path;
}

export function getDownloadBucket(dl: number) {
  switch (true) {
    case dl < 1000:
      return null;
    case dl < 5000:
      return hotT1;
    case dl < 25000:
      return hotT2;
    case dl < 1000000:
      return hotT3;
    default:
      return hotT4;
  }
}

const RELATED_PATH_ALIASES: Record<string, Array<string>> = {
  "/advanced": [`/protocols`, `/protocol`],
  "/getting-started": [`/migration`, `/corepack`],
};

function normalize(path: string) {
  return path.replace(/\/+$/, ``) || `/`;
}

function isPathOrChild(currentPath: string, href: string) {
  return (
    currentPath === href || // Direct path match
    currentPath.startsWith(href) || // Child path match
    currentPath.startsWith(`/${href.split(`/`).at(1)!}`) // Base path match // Ignoring 0 as empty string
  );
}

function isRelatedPath(currentPath: string, href: string) {
  return Object.entries(RELATED_PATH_ALIASES).some(
    ([path, related]) =>
      href.startsWith(path) && // Direct or child path of the true path
      related.some(rel => currentPath.startsWith(rel)),
  );
}

export function isActivePath(currentPath: string, href: string): boolean {
  const target = normalize(currentPath);
  const normalizedHref = normalize(href);

  if (isPathOrChild(target, normalizedHref)) return true;

  return isRelatedPath(target, normalizedHref);
}
