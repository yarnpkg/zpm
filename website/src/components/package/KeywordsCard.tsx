export function KeywordsCard({keywords}: {keywords: Array<string>}) {
  if (keywords.length === 0) return null;

  return (
    <div className={`border border-[var(--line)] rounded-xl bg-[var(--card)] backdrop-blur-lg p-4`}>
      <div className={`flex items-center justify-between mb-3`}>
        <span className={`mono text-[10.5px] tracking-[0.12em] text-[var(--fg-mute)] uppercase`}>Keywords</span>
        <span className={`text-[11.5px] text-[var(--fg-mute)]`}>{keywords.length}</span>
      </div>
      <div className={`flex flex-wrap gap-1.5`}>
        {keywords.map(kw => (
          <span
            key={kw}
            className={`mono text-[11px] py-1 px-2.5 rounded-full border border-[var(--line)] text-[var(--fg-dim)] cursor-pointer transition-all hover:border-[var(--accent-line)] hover:text-[var(--accent)] hover:bg-[var(--accent-soft)]`}
          >
            {kw}
          </span>
        ))}
      </div>
    </div>
  );
}
