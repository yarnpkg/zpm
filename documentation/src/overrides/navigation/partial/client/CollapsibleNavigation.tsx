import {Collapsible}                      from '@base-ui-components/react/collapsible';
import {Separator}                        from '@base-ui-components/react/separator';
import {QueryClient, QueryClientProvider} from '@tanstack/react-query';
import {Suspense}                         from 'preact/compat';
import {useYarnVersions}                  from 'src/api/versions';
import ChevronDown                        from 'src/assets/svg/chevron-down.svg?react';

const queryClient = new QueryClient();

// We did this because of missing types in Preact
const CollapsibleRoot = Collapsible.Root as any;
const CollapsibleTrigger = Collapsible.Trigger as any;
const CollapsiblePanel = Collapsible.Panel as any;

export default function DropdownMenu() {
  return (
    <QueryClientProvider client={queryClient}>
      <Suspense fallback={<LoadingMenu />}>
        <ResponsiveMenu />
      </Suspense>
    </QueryClientProvider>
  );
}

const LoadingMenu = () => (
  <div className={`flex items-center gap-x-1.5`}>
    <div className={`h-4 w-24 bg-white/20 rounded animate-pulse`}></div>
    <ChevronDown className={`size-4 shrink-0 stroke-white`} />
  </div>
);

const ResponsiveMenu = () => {
  const versions = useYarnVersions();

  return (
    <CollapsibleRoot>
      <CollapsibleTrigger className={`group flex items-center gap-x-1.5`}>
        <p className={`text-white text-sm font-medium leading-5`}>Versions</p>
        <ChevronDown className={`size-4 transition-all ease-out shrink-0 group-data-[panel-open]:rotate-180 stroke-white`} />
      </CollapsibleTrigger>

      <CollapsiblePanel className={`mt-3! bg-linear-to-b from-white/15 to-white/5 p-px rounded-xl`}>
        <div className={`rounded-xl py-1.5 px-3 bg-linear-to-b from-gray-950 to-gray-800`}>
          {versions.map(
            ({label, href}: {label: string, href: string}, index) => (
              <>
                <div key={label} className={`p-2`}>
                  <a
                    href={href}
                    aria-label={`Version - ${label}`}
                    target={`_blank`}
                    rel={`noopener noreferrer`}
                    className={`text-white/90 text-sm font-medium leading-5`}
                  >
                    {label}
                  </a>
                </div>
                {index < versions.length - 1 && (
                  <Separator className={`bg-white/15 h-px`} />
                )}
              </>
            ),
          )}
        </div>
      </CollapsiblePanel>
    </CollapsibleRoot>
  );
};
