import {usePackageInfo}    from '@/api/package';
import cn                  from '@/utils/cn';
import {formatPackageLink} from '@/utils/helpers';
import semver              from 'semver';

import Collapsable         from './Collapsable';

interface TagsProps {
  name: string;
}

export default function Tags({name}: TagsProps) {
  const packageData = usePackageInfo(name);

  if (packageData.error) return null;

  const tags = packageData[`dist-tags`];

  const sortedTags = Object.entries(tags).sort((a, b) =>
    semver.rcompare(a[1], b[1]),
  );

  return (
    <Collapsable label={`Tags`} id={`tag-list`} initialCollapsed={false}>
      {sortedTags.map(([tag, version]) => (
        <li key={tag}>
          <button
            onClick={() => {
              window.history.pushState(
                {},
                ``,
                formatPackageLink(name, version),
              );
              window.dispatchEvent(new PopStateEvent(`popstate`));
            }}
            className={`transition-colors hover:underline hover:text-blue-50 text-white/80 underline-offset-2 font-medium! leading-7 text-left line-clamp-1`}
          >
            <span className={cn(tag === `latest` && `text-green-400`)}>
              {tag}
            </span>{` `}
            ({version})
          </button>
        </li>
      ))}
    </Collapsable>
  );
}
