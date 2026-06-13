export interface VersionManifest {
  name: string;
  version: string;
  description?: string;
  dependencies?: Record<string, string>;
  peerDependencies?: Record<string, string>;
  devDependencies?: Record<string, string>;
  dist: {
    tarball: string;
    shasum: string;
    unpackedSize?: number;
    fileCount?: number;
    integrity?: string;
  };
}

export interface RegistryData {
  name: string;
  description: string;
  'dist-tags': Record<string, string>;
  versions: Record<string, VersionManifest>;
  time: Record<string, string>;
  maintainers: Array<{name: string, email: string}>;
  keywords?: Array<string>;
  repository?: {type: string, url: string} | string;
  homepage?: string;
  license?: string | {type: string, url?: string};
  readme?: string;
  bugs?: {url: string} | string;
}

export interface FileEntry {
  name: string;
  hash: string;
  size: number;
}

export interface TreeNode {
  name: string;
  path: string;
  size?: number;
  children?: Array<TreeNode>;
}

export interface DownloadDay {
  downloads: number;
  day: string;
}

export type Tab = `readme` | `versions` | `files` | `audit`;
export type PmTab = `yarn` | `npm` | `pnpm` | `bun`;

export interface IconData {
  body: string;
  width: number;
  height: number;
}

export interface BrandIcons {
  github: IconData;
  npm: IconData;
  yarn: IconData;
  pnpm: IconData;
  bun: IconData;
}

export type OcticonName = `package` | `info` | `globe` | `file-directory` | `file-directory-fill` | `file-directory-open-fill` | `versions` | `file` | `file-code` | `copy` | `chevron-down` | `chevron-right` | `shield` | `law` | `repo` | `home` | `diff` | `link-external` | `x`;
export type Octicons = Record<OcticonName, IconData>;

export interface ParsedUrl {
  name: string;
  version?: string;
  compareVersion?: string;
  tab?: Tab;
  filePath?: string;
}
