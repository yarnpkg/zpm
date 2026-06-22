import type {FileEntry, VersionManifest} from './types';
import {formatBytes, formatDate}         from './utils';

function StatCell({label, value, unit}: {label: string, value: string, unit?: string}) {
  const parts = value.match(/^([\d.,]+)\s*(.*)$/);

  const num = parts ? parts[1] : value;
  const suffix = parts ? parts[2] : unit;

  return (
    <div className={`border-r border-b border-[var(--line)] py-3.5 px-4`}>
      <div className={`mono text-[10px] tracking-[0.12em] text-[var(--fg-mute)] uppercase mb-1.5`}>{label}</div>

      <div className={`text-lg text-[var(--fg)] font-medium tracking-[-0.01em] flex items-baseline gap-1.5`}>
        {num}
        {suffix && <span className={`mono text-[11px] text-[var(--fg-mute)] font-normal`}>{suffix}</span>}
      </div>
    </div>
  );
}

export function StatGrid({versionData, time, version, files}: {
  versionData: VersionManifest | undefined;
  time: Record<string, string>;
  version: string;
  files: Array<FileEntry> | null;
}) {
  const unpackedSize = versionData?.dist?.unpackedSize;
  const totalSize = files ? files.reduce((s, f) => s + f.size, 0) : unpackedSize;
  const depCount = versionData?.dependencies ? Object.keys(versionData.dependencies).length : 0;
  const pubDate = time[version];

  return (
    <div className={`grid grid-cols-4 max-sm:grid-cols-2 border-t border-l border-[var(--line)] mb-7`}>
      <StatCell label={`Install size`} value={totalSize ? formatBytes(totalSize) : `—`}/>
      <StatCell label={`Unpacked`} value={unpackedSize ? formatBytes(unpackedSize) : `—`}/>
      <StatCell label={`Direct deps`} value={String(depCount)} unit={`pkg`}/>
      <StatCell label={`Published`} value={pubDate ? formatDate(pubDate) : `—`}/>
    </div>
  );
}
