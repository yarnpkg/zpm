import {liteClient as algoliasearch}     from 'algoliasearch/lite';
import {history}                         from 'instantsearch.js/es/lib/routers';
import {useEffect}                       from 'preact/hooks';
import {InstantSearch, useInstantSearch} from 'react-instantsearch';

import NoPackagesFound                   from './NoPackagesFound';
import PackageGridSkeleton               from './PackageGridSkeleton';
import PackageSearchInput                from './PackageSearchInput';
import PackageSearchResults              from './PackageSearchResults';

const connection = (navigator as any).connection;

let timerId: any = undefined;
let timeout = 0;

updateTimeout();

export default function SearchPackageInterface() {
  return (
    <InstantSearch
      searchClient={searchClient as any}
      indexName={`npm-search`}
      routing={{
        router: history({
          createURL: ({routeState, location}) => {
            const query = routeState[`npm-search`]?.query;
            const pathname = location.pathname;

            if (pathname.startsWith(`/search`))
              return query ? `/search?q=${query}` : `/search?q=`;

            return ``;
          },

          windowTitle(routeState) {
            const query = routeState[`npm-search`]?.query;

            return query ? `Package Search | Yarn` : ``;
          },

          parseURL({location}) {
            const params = new URLSearchParams(location.search.slice(1));

            return {
              "npm-search": {
                query: params.get(`q`) || ``,
              },
            };
          },
        }),
      }}
    >
      {(<SearchInterface />) as any}
    </InstantSearch>
  );
}

function SearchInterface() {
  const isSearchPage = location.pathname.startsWith(`/search`);

  useEffect(() => {
    connection.addEventListener(`change`, updateTimeout);

    return () => connection.removeEventListener(`change`, updateTimeout);
  });

  return (
    <>
      <PackageSearchInput queryHook={queryHook} />

      {isSearchPage && (
        <NoResultsBoundary fallback={<NoPackagesFound />}>
          <PackageGridSkeleton />
          <PackageSearchResults />
        </NoResultsBoundary>
      )}
    </>
  );
}

function NoResultsBoundary({children, fallback}: any) {
  const {__isArtificial, nbHits} = useInstantSearch().results;

  if (!__isArtificial && nbHits === 0)
    return fallback;


  return children;
}

function queryHook(query: string, refine: (query: string) => void) {
  clearTimeout(timerId);
  timerId = setTimeout(() => refine(query), timeout);
}

function updateTimeout() {
  timeout = [`slow-2g`, `2g`].includes(connection?.effectiveType) ? 400 : 0;
}
