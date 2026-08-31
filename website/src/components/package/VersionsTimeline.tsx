import {useState}                                                       from 'react';

import {useIcons}                                                       from './contexts';
import {OctIcon}                                                        from './icons';
import {formatDate, versionLabel, compareSemverDesc, isNoisyPrerelease} from './utils';

export function VersionsTimeline({versions, distTags, time}: {
  versions: Array<string>; distTags: Record<string, string>; time: Record<string, string>;
}) {
  const {oct} = useIcons();

  const [showAll, setShowAll] = useState(false);
  const [includeNoisy, setIncludeNoisy] = useState(false);

  const sorted = (includeNoisy ? versions.slice() : versions.filter(v => !isNoisyPrerelease(v))).sort(compareSemverDesc);
  const displayed = showAll ? sorted : sorted.slice(0, 15);

  return (
    <article className={`border border-[var(--line-strong)] rounded-[14px] bg-[var(--card)] backdrop-blur-lg`}>
      <header className={`flex items-center gap-2.5 py-3.5 px-[18px] border-b border-[var(--line)]`}>
        <span className={`w-6 h-6 inline-flex items-center justify-center border border-[var(--line-strong)] rounded-md text-[var(--fg-mute)]`}>
          <OctIcon icon={oct.versions} size={12}/>
        </span>
        <span className={`mono text-[11.5px] tracking-[0.1em] text-[var(--fg-dim)] uppercase`}>Release timeline</span>
        <div
          className={`ml-auto flex items-center gap-[6px] cursor-pointer text-[11px] text-[var(--fg-mute)] select-none`}
          onClick={() => setIncludeNoisy(!includeNoisy)}
        >
          <span className={`inline-flex items-center justify-center w-[14px] h-[14px] rounded border text-[9px] leading-none flex-shrink-0 ${
            includeNoisy
              ? `border-[var(--accent)] bg-[var(--accent)] text-[var(--bg-0)]`
              : `border-[var(--fg-mute)] bg-transparent text-transparent`
          }`}>
            {includeNoisy && `\u2713`}
          </span>
          Show all versions
        </div>
      </header>
      <div className={`py-3.5`}>
        {displayed.map((v, i) => {
          const prev = i < displayed.length - 1 ? displayed[i + 1] : null;
          const label = versionLabel(v, prev);
          const isMajor = label === `major` || label === `initial`;
          const tag = Object.entries(distTags).find(([, ver]) => ver === v)?.[0];

          return (
            <div key={v} className={`grid grid-cols-[100px_18px_1fr] gap-4 py-3.5 px-4 border-b border-[var(--line)] last:border-b-0`}>
              <div className={`mono text-[11px] text-[var(--fg-mute)] pt-0.5`}>
                {time[v] ? formatDate(time[v]) : ``}
              </div>
              <div className={`flex flex-col items-center`}>
                <div className={`w-2.5 h-2.5 rounded-full mt-1.5 ${
                  isMajor ? `bg-[var(--accent)] shadow-[0_0_0_4px_var(--accent-soft)]` : `bg-[var(--fg-mute)]`
                }`}/>
                {i < displayed.length - 1 && <div className={`flex-1 w-px bg-[var(--line)] mt-1.5`}/>}
              </div>
              <div>
                <div className={`mono text-sm text-[var(--fg)] mb-1 flex items-center gap-2.5`}>
                  {v}
                  {tag && (
                    <span className={`font-sans text-[9.5px] tracking-[0.1em] uppercase py-0.5 px-1 rounded bg-[var(--accent-soft)] text-[var(--accent)]`}>{tag}</span>
                  )}
                </div>
                <div className={`text-[13px] text-[var(--fg-dim)] leading-relaxed`}>
                  <span className={`mono text-[10.5px] uppercase tracking-[0.06em] mr-1.5 ${
                    label === `major` || label === `initial` ? `text-[oklch(0.78_0.15_25)]`
                      : label === `minor` ? `text-[oklch(0.78_0.13_145)]`
                        : `text-[var(--fg-mute)]`
                  }`}>{label}</span>
                </div>
              </div>
            </div>
          );
        })}
        {!showAll && sorted.length > 15 && (
          <button
            onClick={() => setShowAll(true)}
            className={`w-[calc(100%-32px)] mx-4 mt-3 py-[7px] border border-dashed border-[var(--line-strong)] rounded-lg bg-transparent text-[var(--fg-dim)] mono text-[11px] cursor-pointer transition-colors hover:text-[var(--fg)] hover:border-[var(--fg-mute)]`}
          >
            show all {sorted.length} versions →
          </button>
        )}
      </div>
    </article>
  );
}
