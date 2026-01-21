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

function InfiniteHits() {
  const {results} = useInfiniteHits();

  return (
    <div className={`h-full flex`}>
      <div className={`rounded-xl overflow-y-auto my-8 p-4 bg-black/30 w-full`}>
        {results?.hits?.map((hit: any) => (
          <a key={hit.objectID} href={`/package/${encodeURIComponent(hit.name)}/${encodeURIComponent(hit.version)}`} className={`flex text-white px-4 py-1 hover:bg-white/5 rounded-md`}>
            <div className={`flex-1 basis-4/12 max-w-[300px] truncate`}>{hit.name}</div>
            <div className={`flex-1 basis-2/12 max-w-[120px] truncate`}>{hit.version}</div>
            <div className={`flex-1 basis-2/12 max-w-[200px] truncate`}>{hit.owner?.name}</div>
            <div className={`flex-1 basis-2/12 max-w-[120px] truncate`}>
              <DownloadBadge
                downloadsLast30Days={hit.downloadsLast30Days}
                humanDownloadsLast30Days={hit.humanDownloadsLast30Days}
              />
            </div>
            <div className={`flex-1 basis-6/12 truncate`}>{hit.description}</div>
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
