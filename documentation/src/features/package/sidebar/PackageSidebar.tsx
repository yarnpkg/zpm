import ArrowDownIcon from "src/assets/svg/arrow-down.svg?react";
import WeeklyDownloadsChart from "./WeeklyDownloadsChart";
import { Suspense, lazy } from "preact/compat";
import { PackageBreadcrumbs } from "../views";

const PackageEntry = lazy(() => import("./PackageEntry"));
const Tags = lazy(() => import("./Tags"));
const Versions = lazy(() => import("./Versions"));
const Files = lazy(() => import("./Files"));

export default function PackageSidebar({
  name,
  version,
}: {
  name: string;
  version: string;
}) {
  const searchQuery = localStorage.getItem("lastSearchQuery") || "";

  return (
    <div class="lg:w-1/4 not-content">
      <div class="flex flex-col gap-y-6">
        <div class="lg:hidden">
          <PackageBreadcrumbs name={name} version={version} />
        </div>

        <a
          href={`/search?q=${encodeURIComponent(searchQuery)}`}
          class="py-1 flex gap-x-2 items-center hover:opacity-80 transition-opacity"
        >
          <ArrowDownIcon class="rotate-90 size-3.5" />
          Back to search
        </a>
        <div>
          <div class="rounded-xl flex justify-between gap-4 items-center bg-white/3 transition flex-wrap p-4 mb-4!">
            <p class="text-xl! font-medium! leading-[1.4em] text-blue-50!">
              {decodeURIComponent(name)}
            </p>
            <p class="text-sm leading-5 text-white/80">{version}</p>
          </div>
          <Suspense
            fallback={
              <div class="p-px bg-linear-to-b from-white/15 to-white/5 !mb-2 animate-pulse h-44 rounded-xl"></div>
            }
          >
            <WeeklyDownloadsChart packageName={name} />
          </Suspense>
        </div>

        <div class="flex flex-col gap-y-2">
          <Suspense
            fallback={<div class="h-16 bg-gray-100/5 rounded-xl"></div>}
          >
            <PackageEntry name={name} version={version} />
          </Suspense>

          <Suspense
            fallback={<div class="h-16 bg-gray-100/5 rounded-xl"></div>}
          >
            <Tags name={name} />
          </Suspense>

          <Suspense
            fallback={<div class="h-16 bg-gray-100/5 rounded-xl"></div>}
          >
            <Versions name={name} currentVersion={version} />
          </Suspense>

          <Suspense
            fallback={<div class="h-16 bg-gray-100/5 rounded-xl"></div>}
          >
            <Files name={name} version={version} />
          </Suspense>
        </div>
      </div>
    </div>
  );
}
