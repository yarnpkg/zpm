export function MaintainersCard({maintainers}: {maintainers: Array<{name: string, email: string}>}) {
  if (maintainers.length === 0) return null;

  const hueForName = (name: string) => {
    let hash = 0;

    for (let i = 0; i < name.length; i++)
      hash = ((hash << 5) - hash + name.charCodeAt(i)) | 0;

    return Math.abs(hash) % 360;
  };

  return (
    <div className={`border border-[var(--line)] rounded-xl bg-[var(--card)] backdrop-blur-lg p-4`}>
      <div className={`flex items-center justify-between mb-3`}>
        <span className={`mono text-[10.5px] tracking-[0.12em] text-[var(--fg-mute)] uppercase`}>Maintainers</span>
        <span className={`text-[11.5px] text-[var(--fg-mute)]`}>{maintainers.length}</span>
      </div>
      {maintainers.map((m, i) => {
        const h = hueForName(m.name);
        const initials = m.name.slice(0, 2).toLowerCase();
        return (
          <div key={i} className={`flex items-center gap-2.5 py-2 border-b border-dashed border-[var(--line)] last:border-b-0`}>
            <div
              className={`w-7 h-7 rounded-full inline-flex items-center justify-center text-[var(--fg)] text-[11px] font-semibold shrink-0`}
              style={{background: `linear-gradient(135deg, oklch(0.55 0.16 ${h}), oklch(0.4 0.14 ${h}))`}}
            >
              {initials}
            </div>
            <span className={`mono text-[12.5px] text-[var(--fg-dim)]`}>{m.name}</span>
            {i === 0 && <span className={`mono text-[10px] text-[var(--fg-mute)] ml-auto`}>owner</span>}
          </div>
        );
      })}
    </div>
  );
}
