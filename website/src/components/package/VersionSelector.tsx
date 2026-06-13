import {useState, useEffect, useRef}                                    from 'react';

import {useIcons}                                                       from './contexts';
import {OctIcon}                                                        from './icons';
import {formatDateShort, timeAgo, compareSemverDesc, isNoisyPrerelease} from './utils';

export function VersionSelector({
  version, distTags, versions, time, onVersionChange,
}: {
  version: string;
  distTags: Record<string, string>;
  versions: Array<string>;
  time: Record<string, string>;
  onVersionChange: (v: string) => void;
}) {
  const {oct} = useIcons();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open)
      return undefined;

    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener(`mousedown`, handler);
    return () => document.removeEventListener(`mousedown`, handler);
  }, [open]);

  const isLatest = distTags.latest === version;
  const age = time[version] ? timeAgo(time[version]) : ``;

  const tagForVersion = (v: string) => Object.entries(distTags).find(([, ver]) => ver === v)?.[0];

  return (
    <div className={`flex flex-col gap-2 items-end shrink-0 pt-1.5 relative`} ref={ref}>
      <span className={`mono text-[10px] tracking-[0.12em] text-[var(--fg-mute)] uppercase`}>version</span>

      <button
        onClick={() => setOpen(!open)}
        className={`inline-flex items-center gap-2.5 py-2 px-3 border border-[var(--line-strong)] rounded-[10px] bg-[var(--card)] cursor-pointer mono text-[13px] text-[var(--fg)] transition-colors hover:border-[var(--fg-mute)]`}
      >
        <span>{version}</span>
        {isLatest && (
          <span className={`font-sans text-[9.5px] tracking-[0.1em] uppercase bg-[var(--accent-soft)] text-[var(--accent)] py-0.5 px-1.5 rounded`}>latest</span>
        )}
        {age && <span className={`text-[var(--fg-mute)] text-[11.5px]`}>{age}</span>}
        <OctIcon icon={oct[`chevron-down`]} size={10}/>
      </button>

      {open && (
        <div
          className={`absolute top-full right-0 mt-2 w-[260px] max-h-[320px] overflow-y-auto bg-[color-mix(in_oklch,var(--bg-1)_90%,transparent)] border border-[var(--line-strong)] rounded-xl shadow-[0_20px_60px_-12px_rgba(0,0,0,0.5)] backdrop-blur-xl z-30 pkg-scroll`}
          style={{animation: `pkg-dropdown-in 0.15s ease-out`}}
        >
          <div className={`p-2`}>
            {versions.filter(v => !isNoisyPrerelease(v)).sort(compareSemverDesc).slice(0, 50).map(v => {
              const tag = tagForVersion(v);
              return (
                <button
                  key={v}
                  onClick={() => {
                    onVersionChange(v); setOpen(false);
                  }}
                  className={`flex items-center gap-2 w-full py-2 px-3 rounded-lg mono text-[12.5px] cursor-pointer bg-transparent border-0 transition-colors ${
                    v === version ? `text-[var(--fg)] bg-[color-mix(in_oklch,var(--accent)_8%,transparent)]` : `text-[var(--fg-dim)] hover:text-[var(--fg)] hover:bg-[color-mix(in_oklch,var(--fg)_5%,transparent)]`
                  }`}
                >
                  <span className={`flex-1 text-left`}>{v}</span>
                  {tag && (
                    <span className={`font-sans text-[9px] tracking-[0.1em] uppercase py-0.5 px-1 rounded ${
                      tag === `latest` ? `bg-[var(--accent-soft)] text-[var(--accent)]` : `bg-[color-mix(in_oklch,oklch(0.78_0.16_280)_18%,transparent)] text-[oklch(0.85_0.13_280)]`
                    }`}>{tag}</span>
                  )}
                  <span className={`text-[10.5px] text-[var(--fg-mute)]`}>{time[v] ? formatDateShort(time[v]) : ``}</span>
                </button>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
