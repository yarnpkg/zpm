import { useReducer } from "preact/hooks";
import { ReportView, PackageHeader } from "./index";
import { Suspense } from "preact/compat";

interface PackagePageProps {
  name: string;
  version: string;
}

const ReportViewFallback = () => (
  <div class="animate-pulse bg-gray-100/5 rounded-lg h-[80vh] w-full"></div>
);

export default function PackagePage({ name, version }: PackagePageProps) {
  const [isEditMode, toggleIsEditMode] = useReducer((value) => {
    return !value;
  }, false);

  return (
    <>
      <PackageHeader
        name={name}
        version={version}
        isEditMode={isEditMode}
        toggleIsEditMode={toggleIsEditMode}
      />

      <Suspense fallback={<ReportViewFallback />}>
        <ReportView name={name} version={version} />
      </Suspense>
    </>
  );
}
