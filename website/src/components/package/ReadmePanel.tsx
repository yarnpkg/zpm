import {useIcons}       from './contexts';
import {OctIcon}        from './icons';
import {renderMarkdown} from './utils';

export function ReadmePanel({readme, name}: {readme: string, name: string}) {
  const {oct} = useIcons();
  return (
    <article className={`border border-[var(--line-strong)] rounded-[14px] bg-[var(--card)] backdrop-blur-lg`}>
      <header className={`flex items-center gap-2.5 py-3.5 px-[18px] border-b border-[var(--line)]`}>
        <span className={`w-6 h-6 inline-flex items-center justify-center border border-[var(--line-strong)] rounded-md text-[var(--fg-mute)]`}>
          <OctIcon icon={oct.file} size={12}/>
        </span>
        <span className={`mono text-[11.5px] tracking-[0.1em] text-[var(--fg-dim)] uppercase`}>README.md</span>
      </header>
      <div className={`pkg-readme`} dangerouslySetInnerHTML={{__html: renderMarkdown(readme)}}/>
    </article>
  );
}
