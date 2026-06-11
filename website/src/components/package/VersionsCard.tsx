import {useState}                                              from 'react';

import {formatDateShort, compareSemverDesc, isNoisyPrerelease} from './utils';

export function VersionsCard({versions, distTags, time, onVersionChange}: {
  versions: Array<string>; distTags: Record<string, string>;
  time: Record<string, string>; onVersionChange: (v: string) => void;
}) {
  const [showAll, setShowAll] = useState(false);

  const sorted = versions.filter(v => !isNoisyPrerelease(v)).sort(compareSemverDesc);
  const displayed = showAll ? sorted : sorted.slice(0, 6);

  const tagForVersion = (v: string) => Object.entries(distTags).find(([, ver]) => ver === v)?.[0];

  return (
    <div className={`border border-[var(--line)] rounded-xl bg-[var(--card)] backdrop-blur-lg p-4`}>
      <div className={`flex items-center justify-between mb-3`}>
        <span className={`mono text-[10.5px] tracking-[0.12em] text-[var(--fg-mute)] uppercase`}>Versions</span>
        <span className={`text-[11.5px] text-[var(--fg-mute)]`}>{versions.length} total</span>
      </div>
      {displayed.map(v => {
        const tag = tagForVersion(v);
        return (
          <div
            key={v}
            onClick={() => onVersionChange(v)}
            className={`grid grid-cols-[1fr_auto] items-center py-2 mono text-[12.5px] text-[var(--fg-dim)] border-b border-dashed border-[var(--line)] last:border-b-0 cursor-pointer transition-colors hover:text-[var(--fg)]`}
          >
            <div className={`flex items-center gap-2`}>
              {v}
              {tag && (
                <span className={`font-sans text-[9.5px] tracking-[0.1em] uppercase py-0 px-1 rounded ${
                  tag === `latest`
                    ? `bg-[var(--accent-soft)] text-[var(--accent)]`
                    : `bg-[color-mix(in_oklch,oklch(0.78_0.16_280)_18%,transparent)] text-[oklch(0.85_0.13_280)]`
                }`}>{tag}</span>
              )}
            </div>
            <span className={`text-[var(--fg-mute)] text-[10.5px]`}>{time[v] ? formatDateShort(time[v]) : ``}</span>
          </div>
        );
      })}
      {!showAll && sorted.length > 6 && (
        <button
          onClick={() => setShowAll(true)}
          className={`w-full mt-2.5 py-[7px] border border-dashed border-[var(--line-strong)] rounded-lg bg-transparent text-[var(--fg-dim)] mono text-[11px] cursor-pointer transition-colors hover:text-[var(--fg)] hover:border-[var(--fg-mute)]`}
        >
          show all {sorted.length} versions →
        </button>
      )}
    </div>
  );
}
