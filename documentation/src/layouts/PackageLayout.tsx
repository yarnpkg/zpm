import {QueryClient, QueryClientProvider} from '@tanstack/react-query';
import NotFound                           from 'src/pages/_404';

import {
  ErrorBoundary,
  lazy,
  LocationProvider,
  Route,
  Router,
} from 'preact-iso';

const PackageSidebar = lazy(
  () => import(`../features/package/sidebar/PackageSidebar`),
);
const PackageContent = lazy(
  () => import(`../features/package/views/PackageContent`),
);

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 1000 * 60 * 5,
      retry: 1,
    },
  },
});

export default function PackageLayout() {
  return (
    <QueryClientProvider client={queryClient}>
      <LocationProvider scope={`package`}>
        <ErrorBoundary>
          <Router>
            <Route
              path={`package/:name/:version/:file?`}
              component={PackageWrapper as any}
            />
            <Route default component={NotFound} />
          </Router>
        </ErrorBoundary>
      </LocationProvider>
    </QueryClientProvider>
  );
}

const PackageWrapper = ({
  name,
  version,
  file,
}: {
  name: string;
  version: string;
  file: string;
}) => {
  return (
    <div class={`container pt-8 lg:pt-12 flex flex-col lg:flex-row lg:gap-x-10 xl:gap-x-20`}>
      <PackageSidebar name={name} version={version} />

      <PackageContent name={name} version={version} file={file} />
    </div>
  );
};
