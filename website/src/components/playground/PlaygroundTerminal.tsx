import '@xterm/xterm/css/xterm.css';

import {useEffect, useRef} from 'react';

import type {Terminal as XtermTerminal} from '@xterm/xterm';

interface Props {
  version: string;
}

const cyan = `\x1b[38;2;125;211;252m`;
const green = `\x1b[38;2;134;239;172m`;
const purple = `\x1b[38;2;216;180;254m`;
const yellow = `\x1b[38;2;253;224;71m`;
const dim = `\x1b[38;2;148;163;184m`;
const bold = `\x1b[1m`;
const reset = `\x1b[0m`;

function renderStaticScreen(term: XtermTerminal, version: string) {
  const displayVersion = version.replace(/\.hash-.+$/, ``);

  term.clear();
  term.write([
    `${dim}yarn playground / browser wasm preview${reset}`,
    ``,
    `${green}$${reset} yarn --version`,
    `${displayVersion}`,
    ``,
    `${green}$${reset} yarn install`,
    `${purple}Yarn ${displayVersion}${reset}`,
    `${cyan}[resolution]${reset} Resolved 42 packages`,
    `${cyan}[fetch]${reset} Reused the global cache`,
    `${cyan}[link]${reset} Linked dependencies`,
    `${yellow}[warn]${reset} Static WASM preview output`,
    `${green}[done]${reset} Completed in 0.43s`,
    ``,
    `${green}$${reset} yarn why react`,
    `${bold}=> Found "react@19.2.5"${reset}`,
    `${dim}info "workspace:demo" depends on it${reset}`,
    ``,
    `${green}$${reset} yarn dlx create-yarn-app demo`,
    `${cyan}[wasm]${reset} Sandbox filesystem ready`,
    `${green}[ready]${reset} /workspace mounted`,
    ``,
    `${green}$${reset} `,
  ].join(`\r\n`));
}

export function PlaygroundTerminal({version}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container)
      return undefined;

    let disposed = false;
    let term: XtermTerminal | null = null;
    let resizeObserver: ResizeObserver | null = null;

    async function start() {
      const [{Terminal}, {FitAddon}] = await Promise.all([
        import(`@xterm/xterm`),
        import(`@xterm/addon-fit`),
      ]);

      if (disposed)
        return;

      const fitAddon = new FitAddon();
      term = new Terminal({
        allowTransparency: true,
        convertEol: true,
        cursorBlink: true,
        cursorStyle: `bar`,
        disableStdin: true,
        fontFamily: `'JetBrains Mono', ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace`,
        fontSize: 13,
        lineHeight: 1.35,
        scrollback: 200,
        theme: {
          background: `#00000000`,
          black: `#0f172a`,
          blue: `#7dd3fc`,
          brightBlack: `#64748b`,
          brightBlue: `#bae6fd`,
          brightCyan: `#a5f3fc`,
          brightGreen: `#bbf7d0`,
          brightMagenta: `#e9d5ff`,
          brightRed: `#fecaca`,
          brightWhite: `#f8fafc`,
          brightYellow: `#fef08a`,
          cursor: `#f8fafc`,
          cyan: `#67e8f9`,
          foreground: `#e8ecff`,
          green: `#86efac`,
          magenta: `#d8b4fe`,
          red: `#fca5a5`,
          selectionBackground: `#7dd3fc44`,
          white: `#e2e8f0`,
          yellow: `#fde047`,
        },
      });

      term.loadAddon(fitAddon);
      term.open(container);

      requestAnimationFrame(() => {
        if (!term || disposed)
          return;

        fitAddon.fit();
        renderStaticScreen(term, version);
      });

      resizeObserver = new ResizeObserver(() => {
        if (!term || disposed)
          return;

        fitAddon.fit();
      });
      resizeObserver.observe(container);
    }

    start();

    return () => {
      disposed = true;
      resizeObserver?.disconnect();
      term?.dispose();
    };
  }, [version]);

  return <div ref={containerRef} className="playground-terminal-mount absolute inset-[20px_22px] min-h-0 min-w-0 max-[560px]:inset-3.5" />;
}
