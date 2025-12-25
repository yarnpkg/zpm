import Badge                                  from '@/components/Badge';
import Heading                                from '@/components/Heading';
import cn                                     from '@/utils/cn';
import {formatPackageLink, getDownloadBucket} from '@/utils/helpers';
import {useInfiniteHits}                      from 'react-instantsearch';

export default function PackageSearchResults() {
  const {hits} = useInfiniteHits().results!;

  return (
    <div className={`grid md:grid-cols-2 lg:grid-cols-3 gap-4 lg:gap-6 !mt-10 md:!mt-8`}>
      {hits.map((hit: any) => (
        <PackageCard key={hit.rev} hit={hit} />
      ))}
    </div>
  );
}

const PackageCard = ({hit}: {hit: any}) => {
  const {
    name,
    owner,
    description,
    objectID,
    version,
    styleTypes,
    humanDownloadsLast30Days,
    downloadsLast30Days,
    types,
  } = hit;

  const listing = name ? formatPackageLink(name, version) : undefined;
  const ownerName = owner?.name || `unknown`;

  return (
    <div
      key={objectID}
      className={`bg-linear-to-b from-white/15 to-white/5 rounded-[20px] p-px !mt-0`}
    >
      <div className={`p-6 bg-linear-to-b from-gray-950 to-gray-800 backdrop-blur-[5.7px] rounded-[20px] h-full`}>
        <a href={listing} className={`flex flex-col justify-between h-full`}>
          <div>
            <div className={`flex items-center gap-3 !mb-3`}>
              <Heading>{name}</Heading>
              <div className={`min-w-12`}>
                <Badge text={version} variant={`package`} />
              </div>
            </div>
            <Badge text={`by ${ownerName}`} variant={`author`} />

            {description ? (
              <div className={`text-white/80 !mt-3 line-clamp-1 xl:line-clamp-2`}>
                {description}
              </div>
            ) : (
              <p className={`leading-[22px] text-base !mt-3 text-white/80`}>
                No description available
              </p>
            )}
          </div>
          <div
            className={cn(
              `flex items-center !mt-10`,
              styleTypes?.length ? `justify-between` : `justify-end`,
            )}
          >
            <TypeBadge types={types} />
            <DownloadBadge
              downloadsLast30Days={downloadsLast30Days}
              humanDownloadsLast30Days={humanDownloadsLast30Days}
            />
          </div>
        </a>
      </div>
    </div>
  );
};

const TypeBadge = ({types}: any) => {
  if (!types)
    return null;

  if (types.ts === `included`) {
    return (
      <div className={`flex mr-2.5 rounded-xs px-1 text-sm font-semibold text-center text-white bg-[#0380d9]`}>
        TS
      </div>
    );
  }

  if (types.definitelyTyped) {
    return (
      <div className={`flex mr-2.5 rounded-xs px-1 text-sm font-semibold text-center text-white bg-[#03c4d9]`}>
        DT
      </div>
    );
  }

  return (
    <div className={`flex mr-2.5 rounded-xs px-1 text-sm font-semibold text-center text-white bg-[#cccccc]`}>
      NT
    </div>
  );
};

const DownloadBadge = ({downloadsLast30Days, humanDownloadsLast30Days}: any) => {
  if (!downloadsLast30Days)
    return null;

  const downloadBucket = getDownloadBucket(downloadsLast30Days);
  if (!downloadBucket)
    return null;

  return (
    <div className={`inline-flex items-center gap-2`}>
      <img
        src={downloadBucket}
        className={`size-3.5 shrink-0`}
        alt={`Download bucket`}
      />
      <div>{humanDownloadsLast30Days}</div>
    </div>
  );
};
