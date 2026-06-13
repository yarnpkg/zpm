import type {Tab} from './types';

export function TabBar({active, onTabChange, readmeLabel, versionCount, fileCount}: {
  active: Tab; onTabChange: (t: Tab) => void;
  readmeLabel: string; versionCount: number; fileCount: number;
}) {
  const tabs: Array<{key: Tab, label: string, num: string}> = [
    {key: `readme`, label: `README`, num: readmeLabel},
    {key: `versions`, label: `Versions`, num: String(versionCount)},
    {key: `files`, label: `Files`, num: String(fileCount)},
    {key: `audit`, label: `Audit`, num: `—`},
  ];

  return (
    <div className={`flex items-end gap-0.5 border-b border-[var(--line)] mt-7 mb-6`}>
      {tabs.map(t => (
        <button
          key={t.key}
          onClick={() => onTabChange(t.key)}
          className={`inline-flex items-center gap-2 py-3 px-4 font-sans text-sm font-medium cursor-pointer bg-transparent border-0 border-b-2 -mb-px transition-colors ${
            active === t.key
              ? `text-[var(--fg)] border-b-[var(--accent)]`
              : `text-[var(--fg-mute)] border-b-transparent hover:text-[var(--fg-dim)]`
          }`}
        >
          {t.label}
          <span className={`mono text-[10.5px] font-normal py-0 px-1.5 rounded ${
            active === t.key
              ? `text-[var(--accent)] bg-[var(--accent-soft)]`
              : `text-[var(--fg-mute)] bg-[color-mix(in_oklch,var(--fg)_5%,transparent)]`
          }`}>{t.num}</span>
        </button>
      ))}
    </div>
  );
}
