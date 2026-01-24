import {getDownloadBucket}                            from '@/utils/helpers';
import {useLayoutEffect}                              from 'preact/hooks';
import {InstantSearch, useInfiniteHits, useSearchBox} from 'react-instantsearch';

import {algoliaClient}                                from './config';

export interface SearchResultsProps {
}

function SearchResultsWatcher() {
  const {refine} = useSearchBox();

  useLayoutEffect(() => {
    if (typeof navigation === `undefined`)
      return () => {};

    function handleUrlChange(e: NavigationEvent | null) {
      const url = new URL(e?.destination.url || window.location.href);

      if (url.pathname === `/search`) {
        refine(url.searchParams.get(`q`) || ``);
      }
    }

    navigation.addEventListener(`navigate`, handleUrlChange);
    handleUrlChange(null);

    return () => {
      navigation.removeEventListener(`navigate`, handleUrlChange);
    };
  }, []);

  return null;
}


const DownloadBadge = ({downloadsLast30Days, humanDownloadsLast30Days}: any) => {
  if (!downloadsLast30Days)
    return null;

  const downloadBucket = getDownloadBucket(downloadsLast30Days);
  if (!downloadBucket)
    return null;

  return (
    <div className={`inline-flex items-center gap-1`}>
      <img
        src={downloadBucket}
        className={`size-3.5 shrink-0`}
        alt={`Download bucket`}
      />
      <div>{humanDownloadsLast30Days}</div>
    </div>
  );
};

function InfiniteHits() {
  const {results} = useInfiniteHits();

  return (
    <div className={`h-full flex`}>
      <div className={`overflow-y-auto lg:my-8 w-full flex flex-col gap-4`}>
        {results?.hits?.map((hit: any) => (
          <a key={hit.objectID} href={`/package/${encodeURIComponent(hit.name)}/${encodeURIComponent(hit.version)}`} className={`bg-linear-to-b from-white/15 to-white/5 rounded-[20px] p-px !mt-0`}>
            <div className={`p-6 bg-linear-to-b from-gray-950 to-gray-800 backdrop-blur-[5.7px] rounded-[20px] h-full text-white text-left text-sm space-y-2`}>
              <div className={`flex`}>
                <div className={`flex-1 basis-2/6 max-w-[300px] font-bold truncate`}>{hit.name}</div>
                <div className={`ml-auto mr-0 flex-none`}>{hit.version}</div>
              </div>
              <div className={`flex`}>
                <div className={`flex-1 basis-2/6 max-w-[200px] truncate`}>by {hit.owner?.name}</div>
                <div className={`ml-auto mr-0 flex-none`}>
                  <DownloadBadge
                    downloadsLast30Days={hit.downloadsLast30Days}
                    humanDownloadsLast30Days={hit.humanDownloadsLast30Days}
                  />
                </div>
              </div>
              <div className={`flex-1 basis-6/12 truncate`}>
                {hit.description}
              </div>
            </div>
          </a>
        ))}
      </div>
    </div>
  );
}

export default function SearchResults() {
  return (
    <InstantSearch searchClient={algoliaClient} indexName={`npm-search`}>
      <SearchResultsWatcher/>
      <InfiniteHits/>
    </InstantSearch>
  );
}
