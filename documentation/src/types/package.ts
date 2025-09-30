export interface PackageInfo {
  error?: string;
  name: string;
  ["dist-tags"]: Record<string, string>;
  versions: Record<string, any>;
  time: Record<string, string>;
  readme: string;
}

export interface ReleaseNpmInfo {
  deprecated?: any;
  main?: any;
  type?: any;
  types?: any;
  typings?: any;
  exports?: any;
  homepage?: any;
  readme?: any;
  repository?: {
    type?: any;
    url?: any;
    directory?: any;
  };
  scripts?: any;
}

export interface ReleaseJsDelivrInfo {
  files: Array<ReleaseFile>;
  fileSet: Set<string>;
}

export interface ReleaseInfo {
  name: string;
  version: string;
  npm: ReleaseNpmInfo;
  jsdelivr: ReleaseJsDelivrInfo;
}

export interface ReleaseFile {
  name: string;
  hash: string;
  time: string;
  size: number;
}

export interface PkgInfo {
  name: string;
  "dist-tags"?: {
    latest?: string | null | undefined;
    [tag: string]: string | null | undefined;
  };
  time: Record<string, string>;
  versions: Record<string, {deprecated?: boolean}>;
  readme?: string;
}

export interface VersionChoice {
  value: string;
  label?: string;
  time: Date;
}

export interface VersionSelectorProps {
  version: string;
  name: string;
  class?: string;
  expandFull?: boolean;
}
