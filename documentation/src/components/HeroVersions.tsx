import {useYarnReleaseVersions}           from '@/api/versions';
import {QueryClient, QueryClientProvider} from '@tanstack/react-query';
import {Suspense}                         from 'preact/compat';

const queryClient = new QueryClient();

type BadgeProps = {
  labelClass: string;
  label: string;
  value: any;
};

function Version({name}: {name: string}) {
  const versions = useYarnReleaseVersions();

  return <>{versions[name]}</>;
}

function Badge({labelClass, label, value}: BadgeProps) {
  return (
    <div class={`rounded-xl flex leading-5 text-xs border border-white/10`}>
      <div class={`rounded-l-xl px-2 ${labelClass}`}>
        {label}
      </div>
      <div class={`rounded-r-xl w-16 px-2 bg-linear-to-t from-gray-950 to-gray-800 text-center text-white`}>
        <Suspense fallback={<></>}>
          {value}
        </Suspense>
      </div>
    </div>
  );
}

function Versions() {
  return (
    <div class={`flex items-center gap-x-4`}>
      <Badge labelClass={`bg-linear-to-t from-green-800 to-green-600 text-white`} label={`Stable`} value={<Version name={`stable`} />} />
      <Badge labelClass={`bg-linear-to-t from-orange-900 to-orange-800 text-white`} label={`Canary`} value={<Version name={`canary`} />} />
    </div>
  );
}

export default function HeroVersions() {
  return (
    <QueryClientProvider client={queryClient}>
      <Versions />
    </QueryClientProvider>
  );
}
