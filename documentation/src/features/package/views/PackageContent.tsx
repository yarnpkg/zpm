import { useEffect, Suspense } from "preact/compat";
import { lazy } from "preact-iso";

const FileView = lazy(() => import("./FileView"));
const PackagePage = lazy(() => import("./PackagePage"));

interface RouteParams {
  name: string;
  version: string;
  file?: string;
}

const LoadingFallback = () => (
  <div class="animate-pulse bg-gray-100/5 rounded-lg h-[80vh] w-full"></div>
);

export default function PackageContent({ name, version, file }: RouteParams) {
  useEffect(() => {
    const title = name ? `${name} - Yarn` : "Yarn - Page Not Found";
    document.title = title;
  }, [name]);

  return (
    <div class="lg:w-3/4">
      <Suspense fallback={<LoadingFallback />}>
        {file ? (
          <FileView name={name} version={version} path={file} />
        ) : (
          <PackagePage name={name} version={version} />
        )}
      </Suspense>
    </div>
  );
}
