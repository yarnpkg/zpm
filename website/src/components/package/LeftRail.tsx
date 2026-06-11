import {NavLink, NavLinkExternal} from './NavLink';
import {useIcons}                 from './contexts';
import {OctIcon, BrandIcon}       from './icons';

export function LeftRail({
  name, version, distTags, homepage, repoUrl,
  activeNav, onNavClick, versionCount, fileCount,
}: {
  name: string; version: string;
  distTags: Record<string, string>;
  homepage: string | undefined;
  repoUrl: string | null;
  activeNav: string;
  onNavClick: (id: string) => void;
  versionCount: number;
  fileCount: number;
}) {
  const {oct, brand} = useIcons();
  return (
    <aside className={`sticky top-[90px] self-start hidden lg:block`}>
      <div className={`flex items-center gap-2.5 p-2.5 px-3 border border-[var(--line)] rounded-xl bg-[var(--card)] mb-[18px]`}>
        <span className={`w-[26px] h-[26px] rounded-[7px] bg-[var(--accent-soft)] text-[var(--accent)] inline-flex items-center justify-center shrink-0`}>
          <OctIcon icon={oct.package} size={14}/>
        </span>
        <div className={`min-w-0`}>
          <div className={`mono text-[13px] text-[var(--fg)] whitespace-nowrap overflow-hidden text-ellipsis`}>{name}</div>
          <div className={`mono text-[10.5px] text-[var(--fg-mute)]`}>@ {version}</div>
        </div>
      </div>

      <div className={`mono text-[10.5px] tracking-[0.12em] text-[var(--fg-mute)] uppercase px-3 mb-2`}>Overview</div>

      <NavLink icon={<OctIcon icon={oct.info} size={14}/>} label={`Information`} id={`info`} active={activeNav === `info`} onClick={onNavClick}/>

      {homepage && (
        <NavLinkExternal icon={<OctIcon icon={oct.globe} size={14}/>} label={`Website`} href={homepage}/>
      )}

      {repoUrl && (
        <NavLinkExternal icon={<BrandIcon icon={brand.github} size={14}/>} label={`Repository`} href={repoUrl}/>
      )}

      <div className={`mono text-[10.5px] tracking-[0.12em] text-[var(--fg-mute)] uppercase px-3 mt-[18px] mb-2`}>Index</div>

      <NavLink icon={<OctIcon icon={oct.versions} size={14}/>} label={`Versions`} id={`versions`} active={activeNav === `versions`} onClick={onNavClick} badge={String(versionCount)}/>

      <NavLink icon={<OctIcon icon={oct[`file-directory`]} size={14}/>} label={`Files`} id={`files`} active={activeNav === `files`} onClick={onNavClick}/>
    </aside>
  );
}
