import cn                  from '@/utils/cn';
import {formatPackageLink} from '@/utils/helpers';
import * as semver         from 'semver';
import {usePackageInfo}    from 'src/api/package';

import Collapsable         from './Collapsable';

interface VersionsProps {
  name: string;
  currentVersion: string;
}

export default function Versions({name, currentVersion}: VersionsProps) {
  const packageData = usePackageInfo(name);

  if (packageData.error) return null;

  const taggedVersionsData = packageData[`dist-tags`];
  const versions = packageData.versions;

  const taggedVersionSet = new Set(Object.values(taggedVersionsData));

  const versionToTagMap = Object.fromEntries(
    Object.entries(taggedVersionsData).map(([key, value]) => [value, key]),
  );

  const filteredVersions = Object.keys(versions)
    .filter(version => {
      const isValid = semver.valid(version);
      const isStable = semver.prerelease(version) === null;
      const isTagged = taggedVersionSet.has(version);
      return isValid && (isStable || isTagged);
    })
    .sort(semver.rcompare);

  return (
    <Collapsable label={`Versions`} id={`version-list`}>
      {filteredVersions.map(version => (
        <li key={version}>
          <button
            onClick={() => {
              window.history.pushState(
                {},
                ``,
                formatPackageLink(name, version),
              );
              window.dispatchEvent(new PopStateEvent(`popstate`));
            }}
            className={cn(
              `font-medium! leading-7 line-clamp-1 text-left`,
              version === currentVersion
                ? `text-blue-50 font-bold underline`
                : `text-white/80 hover:underline hover:text-blue-50`,
            )}
          >
            <span>{version}</span>
            {taggedVersionSet.has(version) && (
              <span className={`text-green-400`}>
                ({versionToTagMap[version]})
              </span>
            )}
          </button>
        </li>
      ))}
    </Collapsable>
  );
}
