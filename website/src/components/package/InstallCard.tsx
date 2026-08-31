import {useState, useCallback} from 'react';

import {useIcons}              from './contexts';
import {OctIcon, BrandIcon}    from './icons';
import type {PmTab}            from './types';
import {PM_COMMANDS}           from './utils';

export function InstallCard({name, pmTab, onPmTabChange}: {
  name: string; pmTab: PmTab; onPmTabChange: (t: PmTab) => void;
}) {
  const {oct, brand} = useIcons();

  const [copied, setCopied] = useState(false);

  const cmd = PM_COMMANDS[pmTab];
  const fullCmd = `${cmd.verb} ${cmd.rest} ${name}`;

  const handleCopy = useCallback(() => {
    navigator.clipboard?.writeText(fullCmd).catch(() => {});
    setCopied(true);
    setTimeout(() => setCopied(false), 1400);
  }, [fullCmd]);

  return (
    <div className={`border border-[var(--line-strong)] rounded-[14px] bg-[var(--card)] backdrop-blur-lg mb-7 overflow-hidden`}>
      <div className={`flex items-center justify-between py-3 px-4 border-b border-[var(--line)]`}>
        <span className={`mono text-[10.5px] tracking-[0.12em] text-[var(--fg-mute)] uppercase`}>// install</span>

        <div className={`inline-flex gap-0.5 border border-[var(--line)] rounded-lg p-0.5`}>
          {([`yarn`, `npm`, `pnpm`, `bun`] as Array<PmTab>).map(pm => (
            <button
              key={pm}
              onClick={() => onPmTabChange(pm)}
              className={`inline-flex items-center gap-1.5 mono text-[11.5px] py-1 px-2.5 rounded-md cursor-pointer border-0 transition-colors ${
                pmTab === pm
                  ? `text-[var(--accent)] bg-[var(--accent-soft)]`
                  : `text-[var(--fg-mute)] bg-transparent hover:text-[var(--fg-dim)]`
              }`}
            >
              <BrandIcon icon={brand[pm]} size={12}/>
              {pm}
            </button>
          ))}
        </div>
      </div>

      <div className={`flex items-center gap-3.5 py-3.5 px-4 mono text-sm border-t border-[var(--term-border)] bg-[var(--term-bg)] backdrop-blur-[12px]`}>
        <span className={`text-[var(--term-prompt)] select-none`}>$</span>
        <span className={`flex-1 text-[var(--term-fg)]`}>
          <span className={`text-[var(--accent)]`}>{cmd.verb}</span>{` `}{cmd.rest} {name}
        </span>
        <button
          onClick={handleCopy}
          className={`inline-flex items-center gap-1.5 py-[7px] px-[11px] border rounded-lg mono text-[11px] cursor-pointer bg-transparent transition-all ${
            copied
              ? `text-[var(--accent)] border-[var(--accent-line)]`
              : `text-[var(--fg-dim)] border-[var(--line)] hover:text-[var(--fg)] hover:border-[var(--line-strong)]`
          }`}
        >
          <OctIcon icon={oct.copy} size={11}/>
          <span>{copied ? `copied` : `copy`}</span>
        </button>
      </div>
    </div>
  );
}
