import {Menu as BaseMenu}                 from '@base-ui-components/react/menu';
import {Separator}                        from '@base-ui-components/react/separator';
import {QueryClient, QueryClientProvider} from '@tanstack/react-query';
import {Suspense}                         from 'preact/compat';
import {useYarnVersions}                  from 'src/api/versions';
import ChevronDown                        from 'src/assets/svg/chevron-down.svg?react';

const queryClient = new QueryClient();

// We did this because of missing types in Preact
const MenuRoot = BaseMenu.Root as any;
const MenuTrigger = BaseMenu.Trigger as any;
const MenuPortal = BaseMenu.Portal as any;
const MenuPositioner = BaseMenu.Positioner as any;
const MenuPopup = BaseMenu.Popup as any;
const MenuItem = BaseMenu.Item as any;

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
    <div className={`h-4 w-20 sm:w-32 bg-white/20 rounded animate-pulse`}></div>
    <ChevronDown className={`size-4 shrink-0 stroke-white`} />
  </div>
);

const ResponsiveMenu = () => {
  const versions = useYarnVersions();
  const [defaultVersion, ...dropdownVersions] = versions;

  return (
    <MenuRoot openOnHover>
      <MenuTrigger
        asChild
        className={`select-none active:text-[#7388FF] data-[popup-open]:text-[#7388FF] text-white`}
      >
        <a
          href={defaultVersion.href}
          aria-label={`Version - ${defaultVersion.label}`}
          target={`_blank`}
          rel={`noopener noreferrer`}
          className={`flex items-center gap-x-1.5 text-sm font-medium leading-5`}
        >
          {defaultVersion.label}
          <ChevronDown className={`size-4 shrink-0 stroke-current active:stroke-[#7388FF] data-[popup-open]:stroke-[#7388FF]`} />
        </a>
      </MenuTrigger>
      <MenuPortal>
        <MenuPositioner className={`z-50`} sideOffset={8}>
          <MenuPopup className={`min-w-44 focus:outline-none focus-visible:outline-none`}>
            <div className={`bg-linear-to-b from-white/15 to-white/5 p-px rounded-xl`}>
              <div className={`rounded-xl py-1.5 px-3 bg-linear-to-b from-gray-950 to-gray-800`}>
                {dropdownVersions.map(
                  ({label, href}: {label: string, href: string}, index) => (
                    <>
                      <MenuItem
                        key={label}
                        className={`transition-colors text-sm p-2 text-white hover:text-[#7388FF] w-full focus:outline-none focus-visible:outline-none data-[focus-visible]:outline-none`}
                      >
                        <a
                          href={href}
                          aria-label={`Version - ${label}`}
                          target={`_blank`}
                          rel={`noopener noreferrer`}
                          className={`block`}
                        >
                          {label}
                        </a>
                      </MenuItem>

                      {index < dropdownVersions.length - 1 && (
                        <Separator className={`bg-white/15 h-px`} />
                      )}
                    </>
                  ),
                )}
              </div>
            </div>
          </MenuPopup>
        </MenuPositioner>
      </MenuPortal>
    </MenuRoot>
  );
};
