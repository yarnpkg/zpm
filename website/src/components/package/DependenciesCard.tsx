export function DependenciesCard({deps, title}: {deps: Record<string, string>, title: string}) {
  const entries = Object.entries(deps);
  if (entries.length === 0) return null;

  return (
    <div className={`border border-[var(--line)] rounded-xl bg-[var(--card)] backdrop-blur-lg p-4`}>
      <div className={`flex items-center justify-between mb-3`}>
        <span className={`mono text-[10.5px] tracking-[0.12em] text-[var(--fg-mute)] uppercase`}>{title}</span>
        <span className={`text-[11.5px] text-[var(--fg-mute)]`}>{entries.length}</span>
      </div>
      {entries.map(([name, range]) => (
        <div key={name} className={`flex items-center gap-2.5 py-2 mono text-[12.5px] border-b border-dashed border-[var(--line)] last:border-b-0`}>
          <a
            href={`/package/${name}`}
            className={`text-[var(--fg-dim)] flex-1 no-underline hover:text-[var(--fg)] cursor-pointer`}
          >
            {name}
          </a>
          <span className={`text-[var(--accent)] text-[11.5px]`}>{range}</span>
        </div>
      ))}
    </div>
  );
}
