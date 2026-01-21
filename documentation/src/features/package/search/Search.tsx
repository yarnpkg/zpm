import Badge                                  from '@/components/Badge';
import Heading                                from '@/components/Heading';
import cn                                     from '@/utils/cn';
import {formatPackageLink, getDownloadBucket} from '@/utils/helpers';
import {liteClient as algoliasearch}          from 'algoliasearch/lite';
import {useEffect, useState}                  from 'preact/hooks';

import NoPackagesFound                        from './NoPackagesFound';

const algoliaClient = algoliasearch(
  `OFCNCOG2CU`,
  `f54e21fa3a2a0160595bb058179bfb1e`,
);

const DEFAULT_PACKAGES = [
  `clipanion`,
  `typescript`,
  `next`,
  `jest`,
  `eslint`,
  `esbuild`,
  `webpack`,
  `ts-node`,
  `typanion`,
];

const ATTRIBUTES_TO_RETRIEVE = [
  `name`,
  `version`,
  `description`,
  `owner`,
  `humanDownloadsLast30Days`,
  `downloadsLast30Days`,
  `objectID`,
  `rev`,
  `styleTypes`,
  `types`,
];

export interface SearchProps {
  query: string;
}

type SearchStatus = `idle` | `loading` | `success` | `error`;

interface SearchState {
  hits: Array<any>;
  status: SearchStatus;
  isDefaultResults: boolean;
}

function getQueryFromUrl() {
  const params = new URLSearchParams(location.search);
  return params.get(`q`) || ``;
}

export default function Search({query: initialQuery}: SearchProps) {
  const [query, setQuery] = useState(initialQuery || getQueryFromUrl());
  const [state, setState] = useState<SearchState>({
    hits: [],
    status: `idle`,
    isDefaultResults: false,
  });

  // Sync with URL changes (e.g., from SearchInput or browser navigation)
  useEffect(() => {
    function handleUrlChange() {
      setQuery(getQueryFromUrl());
    }

    // Listen for popstate (back/forward navigation)
    window.addEventListener(`popstate`, handleUrlChange);

    // Listen for Astro view transitions
    document.addEventListener(`astro:after-swap`, handleUrlChange);

    return () => {
      window.removeEventListener(`popstate`, handleUrlChange);
      document.removeEventListener(`astro:after-swap`, handleUrlChange);
    };
  }, []);

  // Also sync when URL changes from navigation (pushState)
  useEffect(() => {
    const originalPushState = history.pushState.bind(history);
    history.pushState = (...args) => {
      originalPushState(...args);
      setQuery(getQueryFromUrl());
    };

    return () => {
      history.pushState = originalPushState;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;

    async function performSearch() {
      setState(prev => ({...prev, status: `loading`}));

      try {
        if (!query || query.trim() === ``) {
          const defaultPackageResults = await Promise.all(
            DEFAULT_PACKAGES.map(packageName =>
              algoliaClient.search([
                {
                  indexName: `npm-search`,
                  params: {
                    query: packageName,
                    hitsPerPage: 1,
                    attributesToRetrieve: ATTRIBUTES_TO_RETRIEVE,
                    attributesToHighlight: [],
                  },
                },
              ]),
            ),
          );

          if (cancelled)
            return;

          const hits = defaultPackageResults.map(({results}) => {
            const result = results[0];
            if (!result)
              return null;

            if (!(`hits` in result))
              return null;

            return result.hits[0];
          }).filter(Boolean);

          setState({
            hits,
            status: `success`,
            isDefaultResults: true,
          });
        } else {
          const response = await algoliaClient.search([
            {
              indexName: `npm-search`,
              params: {
                query,
                hitsPerPage: 20,
                attributesToRetrieve: ATTRIBUTES_TO_RETRIEVE,
                attributesToHighlight: [],
              },
            },
          ]);

          if (cancelled)
            return;

          const result = response.results[0];
          const hits = result && `hits` in result ? result.hits : [];

          setState({
            hits,
            status: `success`,
            isDefaultResults: false,
          });
        }
      } catch (error) {
        if (cancelled)
          return;

        console.error(`Error performing search:`, error);
        setState({
          hits: [],
          status: `error`,
          isDefaultResults: false,
        });
      }
    }

    performSearch();

    return () => {
      cancelled = true;
    };
  }, [query]);

  if (state.status === `loading` || state.status === `idle`) {
    return <PackageGridSkeleton />;
  }

  if (state.status === `error`) {
    return <NoPackagesFound />;
  }

  if (state.hits.length === 0 && !state.isDefaultResults) {
    return <NoPackagesFound />;
  }

  return (
    <div className={`grid md:grid-cols-2 lg:grid-cols-3 gap-4 lg:gap-6 !mt-10 md:!mt-8`}>
      {state.hits.map((hit: any) => (
        <PackageCard key={hit.rev} hit={hit} />
      ))}
    </div>
  );
}

function PackageGridSkeleton() {
  return (
    <div className={`grid md:grid-cols-2 lg:grid-cols-3 gap-4 lg:gap-6 !mt-10 md:!mt-8`}>
      {Array.from({length: 9}).map((_, i) => (
        <div
          key={i}
          className={`bg-linear-to-b from-white/15 to-white/5 rounded-[20px] p-px animate-pulse !mt-0`}
        >
          <div className={`p-6 bg-linear-to-b from-gray-950 to-gray-800 backdrop-blur-[5.7px] rounded-[20px] h-full`}>
            <div className={`flex flex-col justify-between h-full`}>
              <div>
                <div className={`flex items-center gap-3 !mb-3`}>
                  <div className={`h-7 w-full bg-white/20 rounded`} />
                  <div className={`h-5 w-10 bg-white/10 rounded`} />
                </div>
                <div className={`h-5 w-20 bg-white/10 rounded`} />
                <div className={`!mt-3 flex flex-col gap-y-3`}>
                  <div className={`h-4 w-full bg-white/10 rounded`} />
                  <div className={`h-4 w-5/6 bg-white/10 rounded`} />
                  <div className={`h-4 w-3/4 bg-white/10 rounded`} />
                </div>
              </div>
              <div className={`flex items-center !mt-10 md:!mt-8 justify-between`}>
                <div className={`h-5 w-8 bg-white/10 rounded`} />
                <div className={`h-5 w-8 bg-white/10 rounded`} />
              </div>
            </div>
          </div>
        </div>
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
              types ? `justify-between` : `justify-end`,
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
