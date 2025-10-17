import {QueryClient, QueryClientProvider} from '@tanstack/react-query';
import {Suspense}                         from 'preact/compat';
import {useYarnReleaseVersions}           from 'src/api/versions';

const queryClient = new QueryClient();

type BadgeProps = {
  labelClass: string;
  label: string;
  value: string;
};

function Badge({labelClass, label, value}: BadgeProps) {
  return (
    <div class={`rounded-lg flex leading-5 text-xs border border-white/10`}>
      <div class={`rounded-l-lg px-2 ${labelClass}`}>
        {label}
      </div>
      <div class={`rounded-r-lg px-2 bg-linear-to-t from-gray-950 to-gray-800 `}>
        {value}
      </div>
    </div>
  );
}

function Versions() {
  const {stable, canary} = useYarnReleaseVersions();

  return (
    <div class={`flex items-center gap-x-2`}>
      <Badge labelClass="bg-linear-to-t from-green-800 to-green-600 text-white" label="Stable" value={stable} />
      <Badge labelClass="bg-linear-to-t from-orange-900 to-orange-800 text-white" label="Canary" value={canary} />
    </div>
  );
}

export default function HeroVersions() {
  return (
    <QueryClientProvider client={queryClient}>
      <Suspense
        fallback={
          <div class={`flex items-center gap-x-2`}>
            <div class={`h-4 w-20 bg-white/20 rounded animate-pulse`} /> |
            <div class={`h-4 w-20 bg-white/20 rounded animate-pulse`} />
          </div>
        }
      >
        <Versions />
      </Suspense>
    </QueryClientProvider>
  );
}
