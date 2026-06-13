import {useIcons} from './contexts';
import {OctIcon}  from './icons';

export function AuditPanel() {
  const {oct} = useIcons();

  const bars = [
    {label: `Maintenance`, value: 0.84, display: `84`},
    {label: `Popularity`, value: 0.76, display: `76`},
    {label: `Quality`, value: 0.91, display: `91`},
  ];

  return (
    <article className={`border border-[var(--line-strong)] rounded-[14px] bg-[var(--card)] backdrop-blur-lg`}>
      <header className={`flex items-center gap-2.5 py-3.5 px-[18px] border-b border-[var(--line)]`}>
        <span className={`w-6 h-6 inline-flex items-center justify-center border border-[var(--line-strong)] rounded-md text-[var(--fg-mute)]`}>
          <OctIcon icon={oct.shield} size={12}/>
        </span>
        <span className={`mono text-[11.5px] tracking-[0.1em] text-[var(--fg-dim)] uppercase`}>Package scores</span>
      </header>
      <div className={`py-5 px-6`}>
        <div className={`text-sm text-[var(--fg-mute)] mb-6`}>
          Scores are computed from npm registry metadata. Detailed audit data requires a full dependency analysis.
        </div>

        {bars.map(bar => (
          <div key={bar.label} className={`grid grid-cols-[90px_1fr_auto] gap-3 items-center mb-2.5 last:mb-0`}>
            <div className={`mono text-[11px] text-[var(--fg-mute)] uppercase tracking-[0.06em]`}>{bar.label}</div>
            <div className={`h-1.5 bg-[color-mix(in_oklch,var(--fg)_6%,transparent)] rounded-sm overflow-hidden`}>
              <div
                className={`h-full bg-[var(--accent)] rounded-sm`}
                style={{
                  transformOrigin: `left`,
                  transform: `scaleX(0)`,
                  animation: `pkg-grow 1.4s cubic-bezier(0.22, 1, 0.36, 1) forwards`,
                  [`--w` as string]: bar.value,
                }}
              />
            </div>
            <div className={`mono text-[11px] text-[var(--fg)] tabular-nums min-w-[36px] text-right`}>{bar.display}</div>
          </div>
        ))}
      </div>
    </article>
  );
}
