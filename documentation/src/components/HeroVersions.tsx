import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Suspense } from "preact/compat";
import { useYarnReleaseVersions } from "src/api/versions";

const queryClient = new QueryClient();

function Versions() {
  const { stable, canary } = useYarnReleaseVersions();

  return (
    <div class="flex items-center gap-x-2 divide-x divide-white">
      <p class="md:text-sm text-xs text-white! leading-5 pr-2">
        stable {stable}
      </p>
      <p class="md:text-sm text-xs text-white! leading-5">canary {canary}</p>
    </div>
  );
}

export default function HeroVersions() {
  return (
    <QueryClientProvider client={queryClient}>
      <Suspense
        fallback={
          <div class="flex items-center gap-x-2">
            <div class="h-4 w-20 bg-white/20 rounded animate-pulse" /> |
            <div class="h-4 w-20 bg-white/20 rounded animate-pulse" />
          </div>
        }
      >
        <Versions />
      </Suspense>
    </QueryClientProvider>
  );
}
