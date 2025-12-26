import type {Check}       from '@/api/packageChecks';
import clsx               from 'clsx';
import type {ChangeEvent} from 'preact/compat';
import {useLocalStorage}  from 'usehooks-ts';

export default function ReportCheckList({
  name,
  version,
  versionData,
  check,
  isEditMode,
}: {
  name: string;
  version: string;
  versionData?: any;
  check: Check;
  isEditMode: boolean;
}) {
  const [isEnabled, setIsEnabled] = useLocalStorage<boolean>(
    `check/${check.id}`,
    check.defaultEnabled,
  );

  const checkResult = check.useCheck({
    name,
    version,
    versionData,
  });

  const statusLabel = checkResult ? (
    <p
      className={clsx(
        `leading-[22px] text-base ${
          checkResult.ok ? `text-green-600` : `text-yellow-600`
        }`,
      )}
    >
      {checkResult.ok ? `Check` : `Alert`}
    </p>
  ) : (
    <p className={`text-white/60 leading-[22px] text-base`}>Glass</p>
  );

  if (!isEditMode && !isEnabled) return null;

  if (!isEditMode && checkResult?.ok && !checkResult.message) return null;

  return (
    <div className={`flex items-center justify-between`}>
      <div className={`pr-4`}>{statusLabel}</div>
      <div>
        {checkResult?.message ??
          (checkResult?.ok ? check.success : check.failure)}
      </div>
      {isEditMode && (
        <div>
          <label>
            <input
              type={`checkbox`}
              onChange={(e: ChangeEvent<HTMLInputElement>) =>
                setIsEnabled((e.target as HTMLInputElement).checked)
              }
              checked={isEnabled}
            />
          </label>
        </div>
      )}
    </div>
  );
}
