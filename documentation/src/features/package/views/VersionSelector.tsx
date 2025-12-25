import {usePackageInfo} from '@/api/package';
import VersionDropdown  from '@/features/package/views/VersionDropdown';
import semver           from 'semver';

import type {
  VersionSelectorProps,
  PkgInfo,
  VersionChoice,
} from '@/types/package';

export default function VersionSelector({
  name,
  version,
}: VersionSelectorProps) {
  const pkgInfo = usePackageInfo(name);

  const versions = getFilteredVersions(pkgInfo);

  const currentVersion = versions.find(({value}) => {
    return value === version;
  });

  return (
    <VersionDropdown
      packageName={pkgInfo.name}
      versions={versions}
      currentVersion={currentVersion!}
    />
  );
}

function getFilteredVersions(pkgInfo: PkgInfo): Array<VersionChoice> {
  const {time = {}, versions = {}} = pkgInfo;
  const latest = pkgInfo[`dist-tags`]?.latest ?? ``;

  const versionEntries = Object.entries(time)
    .filter(([v]) => v !== `created` && v !== `modified`)
    .map(([v, releaseTime]) => ({
      value: v,
      time: new Date(releaseTime as string),
    }));

  return versionEntries
    .sort((a, b) => b.time.getTime() - a.time.getTime())
    .filter(({value}, index) =>
      isValidVersion(value, versions, latest, index),
    );
}

function isValidVersion(
  version: string,
  versionsData: any,
  latest: string,
  index: number,
): boolean {
  const info = versionsData[version];

  if (info?.deprecated) return false;

  if (semver.prerelease(version) && semver.gt(latest, version)) return false;

  const isNightly = /-.*2[0-9]{3}(0[1-9]|1[0-2])(0[1-9]|[12][0-9]|3[01])/.test(
    version,
  );
  if (isNightly && index > 0) return false;

  return true;
}
