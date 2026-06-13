export function NavLink({icon, label, id, active, onClick, badge}: {
  icon: React.ReactNode; label: string; id: string; active: boolean;
  onClick: (id: string) => void; badge?: string;
}) {
  return (
    <button
      onClick={() => onClick(id)}
      className={`flex items-center gap-2.5 w-full text-left py-2 px-3 rounded-lg text-[13.5px] cursor-pointer bg-transparent border-0 border-l -ml-0.5 transition-colors ${
        active
          ? `text-[var(--accent)] border-l-[var(--accent)] font-medium`
          : `text-[var(--fg-dim)] border-l-transparent hover:text-[var(--fg)] hover:bg-[color-mix(in_oklch,var(--fg)_4%,transparent)]`
      }`}
    >
      <span className={active ? `text-[var(--accent)]` : `text-[var(--fg-mute)]`}>{icon}</span>
      <span className={`flex-1`}>{label}</span>
      {badge && <span className={`text-[10px] text-[var(--fg-mute)]`}>{badge}</span>}
    </button>
  );
}

export function NavLinkExternal({icon, label, href}: {icon: React.ReactNode, label: string, href: string}) {
  return (
    <a
      href={href}
      target={`_blank`}
      rel={`noopener noreferrer`}
      className={`flex items-center gap-2.5 w-full py-2 px-3 rounded-lg text-[13.5px] text-[var(--fg-dim)] no-underline border-l-2 border-l-transparent -ml-0.5 transition-colors hover:text-[var(--fg)] hover:bg-[color-mix(in_oklch,var(--fg)_4%,transparent)]`}
    >
      <span className={`text-[var(--fg-mute)]`}>{icon}</span>
      <span className={`flex-1`}>{label}</span>
      <span className={`text-[10px] text-[var(--fg-mute)]`}>↗</span>
    </a>
  );
}
