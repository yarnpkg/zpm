import cn                   from '@/utils/cn';
import {Suspense}           from 'preact/compat';
import {checks}             from '@/api/packageChecks';
import GearIcon             from '@/assets/svg/gear.svg?react';
import CheckList            from '@/features/package/views/Checklist';

import VersionSelector      from './VersionSelector';
import {PackageBreadcrumbs} from '.';

export default function PackageHeader({
  name,
  version,
  isEditMode,
  toggleIsEditMode,
}: {
  name: string;
  version: string;
  isEditMode: boolean;
  toggleIsEditMode: (value: boolean) => void;
}) {
  return (
    <div>
      <div class={`max-lg:hidden`}>
        <PackageBreadcrumbs name={name} version={version} />
      </div>
      <div class={`flex xl:flex-row flex-col justify-between xl:items-center gap-x-4`}>
        <p class={`text-base bg-linear-to-b from-[#656E98] to-white to-60% text-transparent bg-clip-text text-[64px] leading-none tracking-[6%] py-2 max-w-2xl break-all`}>
          {name}
        </p>
        <div class={`flex-1 flex justify-end`}>
          <div class={`flex items-center gap-x-3 w-full md:w-96`}>
            <div class={`flex-1 flex items-center justify-end`}>
              <VersionSelector name={name} version={version} />
            </div>

            <button
              type={`button`}
              onClick={e => toggleIsEditMode(!e)}
              className={`rounded-md size-max bg-linear-to-b from-white/15 to-white/5 p-px !m-0`}
            >
              <div class={`bg-linear-to-b flex items-center justify-center from-[#181A1F] to-[#0D0F14] rounded-md p-3`}>
                <GearIcon class={cn(`transition`, isEditMode && `rotate-90`)} />
              </div>
            </button>
          </div>
        </div>
      </div>

      <Suspense
        fallback={
          <div class={`animate-pulse bg-gray-100/5 rounded-lg h-16 w-full`}></div>
        }
      >
        <CheckList
          checks={checks}
          editMode={isEditMode}
          name={name}
          version={version}
        />
      </Suspense>
    </div>
  );
}
