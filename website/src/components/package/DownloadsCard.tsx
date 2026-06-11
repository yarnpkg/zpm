import type {DownloadDay}                from './types';
import {formatNumberFull, sparklinePath} from './utils';

export function DownloadsCard({downloads}: {downloads: Array<DownloadDay> | null}) {
  if (!downloads || downloads.length === 0) return null;

  const lastWeek = downloads.slice(-7).reduce((s, d) => s + d.downloads, 0);
  const prevWeek = downloads.slice(-14, -7).reduce((s, d) => s + d.downloads, 0);

  const pctChange = prevWeek > 0 ? ((lastWeek - prevWeek) / prevWeek * 100) : 0;

  const dailyData = downloads.map(d => d.downloads);
  const startDate = downloads[0]?.day;
  const endDate = downloads[downloads.length - 1]?.day;

  const W = 280;
  const H = 56;

  const line = sparklinePath(dailyData, W, H);
  const area = line ? `${line} L${W},${H} L0,${H} Z` : ``;

  return (
    <div className={`border border-[var(--line)] rounded-xl bg-[var(--card)] backdrop-blur-lg p-4`}>
      <div className={`flex items-center justify-between mb-3`}>
        <span className={`mono text-[10.5px] tracking-[0.12em] text-[var(--fg-mute)] uppercase`}>Weekly downloads</span>
        <span className={`text-[11.5px] text-[var(--fg-mute)]`}>all versions</span>
      </div>

      <div className={`text-[28px] font-medium text-[var(--fg)] tracking-[-0.02em] tabular-nums mb-1`}>
        {formatNumberFull(lastWeek)}
        {pctChange !== 0 && (
          <span className={`text-xs ml-2 font-normal ${pctChange > 0 ? `text-[oklch(0.78_0.16_145)]` : `text-[oklch(0.78_0.16_25)]`}`}>
            {pctChange > 0 ? `+` : ``}{pctChange.toFixed(1)}%
          </span>
        )}
      </div>

      <div className={`mono text-[10.5px] text-[var(--fg-mute)] mb-3`}>
        {startDate} → {endDate}
      </div>

      {line && (
        <>
          <svg className={`w-full h-14 block`} viewBox={`0 0 ${W} ${H}`} preserveAspectRatio={`none`}>
            <defs>
              <linearGradient id={`pkg-spark-g`} x1={`0`} x2={`0`} y1={`0`} y2={`1`}>
                <stop offset={`0%`} stopColor={`var(--accent)`} stopOpacity={`0.45`}/>
                <stop offset={`100%`} stopColor={`var(--accent)`} stopOpacity={`0`}/>
              </linearGradient>
            </defs>
            <path d={area} fill={`url(#pkg-spark-g)`}/>
            <path d={line} fill={`none`} stroke={`var(--accent)`} strokeWidth={`1.5`}/>
          </svg>
          <div className={`flex justify-between mono text-[10px] text-[var(--fg-mute)] mt-1`}>
            <span>{startDate ? new Date(startDate).toLocaleDateString(`en-US`, {month: `short`, day: `numeric`}) : ``}</span>
            <span>{endDate ? new Date(endDate).toLocaleDateString(`en-US`, {month: `short`, day: `numeric`}) : ``}</span>
          </div>
        </>
      )}
    </div>
  );
}
