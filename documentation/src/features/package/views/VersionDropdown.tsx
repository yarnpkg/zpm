import {formatDate, formatPackageLink} from '@/utils/helpers';
import {Menu as BaseMenu}              from '@base-ui-components/react/menu';
import ChevronDown                     from 'src/assets/svg/chevron-down.svg?react';

interface VersionChoice {
  value: string;
  time: Date;
}

const BaseMenuRoot = BaseMenu.Root as any;
const BaseMenuTrigger = BaseMenu.Trigger as any;
const BaseMenuPortal = BaseMenu.Portal as any;
const BaseMenuPositioner = BaseMenu.Positioner as any;
const BaseMenuPopup = BaseMenu.Popup as any;
const BaseMenuItem = BaseMenu.Item as any;

export default function ResponsiveMenu({
  versions,
  packageName,
  currentVersion,
}: {
  versions: Array<VersionChoice>;
  packageName: string;
  currentVersion: VersionChoice;
}) {
  if (!currentVersion) return null;

  return (
    <CustomDropdown
      versions={versions}
      packageName={packageName}
      currentVersion={currentVersion}
    />
  );
}

const CustomDropdown = ({
  versions,
  packageName,
  currentVersion,
}: {
  versions: Array<VersionChoice>;
  packageName: string;
  currentVersion: VersionChoice;
}) => {
  const handleVersionSelect = (versionValue: string) => {
    window.history.pushState(
      {},
      ``,
      formatPackageLink(packageName, versionValue),
    );
    window.dispatchEvent(new PopStateEvent(`popstate`));
  };

  return (
    <BaseMenuRoot>
      <BaseMenuTrigger className={`bg-linear-to-b w-full from-white/15 to-white/5 p-px rounded-md focus:outline-none focus-visible:outline-none`}>
        <div class={`flex items-center group justify-between bg-linear-to-b from-[#181A1F] to-[#0D0F14] rounded-md gap-x-4 px-4 py-2 text-xs sm:text-sm text-white transition duration-200 !font-medium hover:text-blue-50`}>
          <div className={`flex w-full items-center gap-x-3`}>
            <span class={`text-xl font-medium break-all line-clamp-1 wrap-break-word`}>
              {currentVersion.value}
            </span>
            <span class={`text-sm wrap-normal leading-5 text-white/80 group-hover:text-blue-50`}>
              {formatDate(currentVersion.time)}
            </span>
          </div>
          <ChevronDown class={`!m-0 size-4 stroke-white`} />
        </div>
      </BaseMenuTrigger>
      <BaseMenuPortal>
        <BaseMenuPositioner className={`z-50`} sideOffset={8}>
          <BaseMenuPopup className={`shadow-lg min-w-[280px] max-w-[400px] focus:outline-none focus-visible:outline-none`}>
            <div className={`bg-gradient-to-b from-white/15 to-white/5 rounded-xl shadow-lg`}>
              <div className={`rounded-xl py-1.5 bg-gradient-to-b from-gray-950 to-gray-800 max-h-100 overflow-y-scroll`}>
                {versions.map((version, index) => (
                  <div key={version.value} className={`block w-full`}>
                    {index > 0 && (
                      <BaseMenu.Separator className={`h-px bg-gradient-to-b from-white/15 to bg-white/5`} />
                    )}

                    <BaseMenuItem
                      onClick={() => handleVersionSelect(version.value)}
                      className={`flex justify-between items-center w-full gap-x-6 rounded px-3 py-2 transition-colors duration-200 text-xs sm:text-sm text-white hover:text-blue-50 focus:outline-none focus-visible:outline-none cursor-pointer`}
                    >
                      <span class={`font-medium break-all`}>{version.value}</span>
                      <span class={`text-sm break-keek word font-light leading-5 text-white/80 group-hover:text-blue-50`}>
                        {formatDate(version.time)}
                      </span>
                    </BaseMenuItem>
                  </div>
                ))}
              </div>
            </div>
          </BaseMenuPopup>
        </BaseMenuPositioner>
      </BaseMenuPortal>
    </BaseMenuRoot>
  );
};
