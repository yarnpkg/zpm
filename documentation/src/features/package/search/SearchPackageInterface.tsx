import { liteClient as algoliasearch } from "algoliasearch/lite";
import { InstantSearch, useInstantSearch } from "react-instantsearch";

import { history } from "instantsearch.js/es/lib/routers";
import PackageSearchInput from "./PackageSearchInput";

import { useEffect } from "preact/hooks";
import PackageGridSkeleton from "./PackageGridSkeleton";
import NoPackagesFound from "./NoPackagesFound";
import PackageSearchResults from "./PackageSearchResults";

const algoliaClient = algoliasearch(
  "OFCNCOG2CU",
  "f54e21fa3a2a0160595bb058179bfb1e"
);

const DEFAULT_PACKAGES = [
  "clipanion",
  "typescript",
  "next",
  "jest",
  "eslint",
  "esbuild",
  "webpack",
  "ts-node",
  "typanion",
];

const searchClient = {
  ...algoliaClient,
  async search(requests: any[]) {
    if (
      requests.every(
        ({ params }: { params: { query: string } }) => !params.query
      )
    ) {
      try {
        const defaultPackageResults = await Promise.all(
          DEFAULT_PACKAGES.map((packageName) =>
            algoliaClient.search([
              {
                indexName: "npm-search",
                params: {
                  query: packageName,
                  hitsPerPage: 1,
                  attributesToRetrieve: [
                    "name",
                    "version",
                    "description",
                    "owner",
                    "humanDownloadsLast30Days",
                    "downloadsLast30Days",
                    "objectID",
                    "rev",
                    "styleTypes",
                    "types",
                  ],
                  attributesToHighlight: [],
                },
              },
            ])
          )
        );

        const hits = defaultPackageResults
          .map((result) => result.results[0].hits[0] ?? [])
          .filter(Boolean);

        return {
          results: requests.map(() => ({
            hits,
            nbHits: hits.length,
            nbPages: 1,
            page: 0,
            processingTimeMS: 1,
            hitsPerPage: hits.length,
            exhaustiveNbHits: false,
            query: "",
            params: "",
            __isArtificial: true,
          })),
        };
      } catch (error) {
        console.error("Error fetching default packages:", error);
        return {
          results: requests.map(() => ({
            hits: [],
            nbHits: 0,
            nbPages: 0,
            page: 0,
            processingTimeMS: 0,
            hitsPerPage: 0,
            exhaustiveNbHits: false,
            query: "",
            params: "",
          })),
        };
      }
    }

    return algoliaClient.search(requests);
  },
};

const connection = (navigator as any).connection;

let timerId: any = undefined;
let timeout = 0;

updateTimeout();

export default function SearchPackageInterface() {
  return (
    <InstantSearch
      searchClient={searchClient as any}
      indexName="npm-search"
      routing={{
        router: history({
          createURL: ({ routeState, location }) => {
            const query = routeState["npm-search"]?.query;
            const pathname = location.pathname;

            if (pathname.startsWith("/search")) {
              return query ? `/search?q=${query}` : `/search?q=`;
            }

            return "";
          },

          windowTitle(routeState) {
            const query = routeState["npm-search"]?.query;

            return query ? "Package Search | Yarn" : "";
          },

          parseURL({ location }) {
            const params = new URLSearchParams(location.search.slice(1));

            return {
              "npm-search": {
                query: params.get("q") || "",
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
  const isSearchPage = location.pathname.startsWith("/search");

  useEffect(() => {
    connection.addEventListener("change", updateTimeout);

    return () => connection.removeEventListener("change", updateTimeout);
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

function NoResultsBoundary({ children, fallback }: any) {
  const { __isArtificial, nbHits } = useInstantSearch().results;

  if (!__isArtificial && nbHits === 0) {
    return fallback;
  }

  return children;
}

function queryHook(query: string, refine: (query: string) => void) {
  clearTimeout(timerId);
  timerId = setTimeout(() => refine(query), timeout);
}

function updateTimeout() {
  timeout = ["slow-2g", "2g"].includes(connection?.effectiveType) ? 400 : 0;
}
