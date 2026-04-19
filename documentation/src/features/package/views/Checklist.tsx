import type {Check}      from '@/api/packageChecks';
import {usePackageInfo}  from '@/api/package';
import CheckIcon         from '@/assets/svg/check-icon.svg?react';
import HourglassIcon     from '@/assets/svg/hourglass-icon.svg?react';
import WarningIcon       from '@/assets/svg/warning-icon.svg?react';
import cn                from '@/utils/cn';
import {useLocalStorage} from 'usehooks-ts';

interface UseCheckDataProps {
  check: Check;
  name: string;
  version: string;
  versionData: any;
  editMode: boolean;
}

export function useCheckData({
  check,
  name,
  version,
  versionData,
  editMode,
}: UseCheckDataProps) {
  const [isEnabled, setIsEnabled] = useLocalStorage<boolean>(
    `check/${check.id}`,
    check.defaultEnabled,
  );

  const result = check.useCheck({name, version, versionData});

  const icon: `check` | `alert` | `glass` = result
    ? result.ok
      ? `check`
      : `alert`
    : `glass`;

  let shouldShow = true;

  if (!editMode && !isEnabled) shouldShow = false;

  if (!editMode && result?.ok && !result.message) shouldShow = false;

  const message =
    result?.message ?? (result?.ok ? check.success : check.failure);

  return {
    id: check.id,
    icon,
    message,
    enabled: isEnabled,
    setEnabled: setIsEnabled,
    show: shouldShow,
  };
}

interface ListItemProps {
  check: Check;
  name: string;
  version: string;
  versionData: any;
  editMode: boolean;
}

export function ListItem({
  check,
  name,
  version,
  versionData,
  editMode,
}: ListItemProps) {
  const data = useCheckData({
    check,
    name,
    version,
    versionData,
    editMode,
  });

  const {icon, message, enabled, setEnabled, show} = data;

  return (
    <div
      class={`w-full hidden package-check [[data-show=true]]:flex items-center m-0!`}
      data-show={show}
      aria-hidden={!show}
    >
      <div class={`flex items-center gap-x-4 justify-between flex-1`}>
        {icon === `check` ? (
          <CheckIcon class={`stroke-green-600 size-5 shrink-0`} />
        ) : icon === `alert` ? (
          <WarningIcon class={`stroke-yellow-600 size-5 shrink-0`} />
        ) : (
          <HourglassIcon class={`stroke-blue-400 size-5 shrink-0`} />
        )}
        <div
          class={`m-0! text-start w-full`}
          dangerouslySetInnerHTML={{
            __html: message as string,
          }}
        />
        <div class={`flex items-center !m-0`}>
          <button
            class={cn(
              `rounded border overflow-hidden transition border-slate-500 hover:bg-blue-600`,
              editMode ? `opacity-100` : `opacity-0 pointer-events-none`,
            )}
            aria-checked={enabled}
            onClick={() => setEnabled((v: boolean) => !v)}
            disabled={!editMode}
            aria-disabled={!editMode}
          >
            <div
              data-show={enabled}
              className={`opacity-0 size-5 transition [[data-show=true]]:opacity-100 bg-blue-600 flex justify-center items-center`}
            >
              <CheckIcon class={`block size-5`} />
            </div>
          </button>
        </div>
      </div>
    </div>
  );
}

interface CheckListProps {
  name: string;
  version: string;
  checks: Array<Check>;
  editMode: boolean;
}

export default function CheckList({
  name,
  version,
  checks,
  editMode,
}: CheckListProps) {
  const pkgInfo = usePackageInfo(name);

  if (!pkgInfo) return null;

  const versionData = pkgInfo.versions[version];

  return (
    <div class={`[:has(.package-check[data-show=true])]:pb-4`}>
      <div
        class={`w-full bg-white/3 [:has(>.package-check[data-show=true])]:border text-white border-white/5 rounded-xl [:has(>.package-check[data-show=true])]:p-4 flex flex-col gap-y-4`}
        id={`checklist`}
      >
        {checks.map(check => (
          <ListItem
            key={check.id}
            check={check}
            name={name}
            version={version}
            versionData={versionData}
            editMode={editMode}
          />
        ))}
      </div>
    </div>
  );
}
