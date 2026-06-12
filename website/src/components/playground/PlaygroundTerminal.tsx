import '@xterm/xterm/css/xterm.css';
import type {Terminal as XtermTerminal} from '@xterm/xterm';
import {useEffect, useRef}              from 'react';

interface Props {
  files: Array<PlaygroundFile>;
  version: string;
}

type BrowserPodApi = {
  boot(opts: {
    apiKey: string;
    storageKey?: string;
  }): Promise<BrowserPodInstance>;
};

type BrowserPodInstance = {
  createCustomTerminal(opts: {
    cols?: number;
    rows?: number;
    onOutput: (buffer: ArrayBuffer) => void;
  }): Promise<BrowserPodTerminal>;
  createDirectory(path: string, opts?: {recursive?: boolean}): Promise<void>;
  createFile(path: string, mode: `binary` | `text`): Promise<BrowserPodFile>;
  run(executable: string, args: Array<string>, opts: {
    cwd?: string;
    echo?: boolean;
    terminal: BrowserPodTerminal;
  }): Promise<unknown>;
};

type BrowserPodFile = {
  close(): Promise<void>;
  write(data: ArrayBuffer | string): Promise<number>;
};

type BrowserPodTerminal = {
  readData(data: string): void;
};

export type PlaygroundFile = {
  content: string;
  path: string;
};

const cyan = `\x1b[38;2;125;211;252m`;
const green = `\x1b[38;2;134;239;172m`;
const purple = `\x1b[38;2;216;180;254m`;
const yellow = `\x1b[38;2;253;224;71m`;
const dim = `\x1b[38;2;148;163;184m`;
const red = `\x1b[38;2;252;165;165m`;
const reset = `\x1b[0m`;

const PROJECT_PATH = `/home/user/yarn-playground`;
const BROWSERPOD_RUNTIME_URL = `https://rt.browserpod.io/2.10.0/browserpod.js`;
const YARN_BIN_PATH = `${PROJECT_PATH}/yarn-bin`;
const YARN_BIN_ASSET = `/browserpod/yarn-bin.wasm`;

function writeLines(term: XtermTerminal, lines: Array<string>) {
  term.write(lines.join(`\r\n`));
}

function writeBootScreen(term: XtermTerminal, version: string) {
  const displayVersion = version.replace(/\.hash-.+$/, ``);

  term.clear();
  writeLines(term, [
    `${dim}yarn playground / BrowserPod runtime${reset}`,
    ``,
    `${cyan}[browserpod]${reset} Loading official BrowserPod 2.10.0 runtime`,
    `${cyan}[browserpod]${reset} Preparing /home/user/yarn-playground`,
    `${purple}[yarn]${reset} Target version ${displayVersion}`,
    ``,
  ]);
}

function getBrowserPodApiKey() {
  const env = import.meta.env as Record<string, string | undefined>;
  return env.PUBLIC_BROWSERPOD_API_KEY ?? env.VITE_BPAPIKEY ?? env.VITE_BP_APIKEY ?? ``;
}

function dirname(path: string) {
  const index = path.lastIndexOf(`/`);
  return index === -1 ? `` : path.slice(0, index);
}

async function writeProjectFiles(pod: BrowserPodInstance, files: Array<PlaygroundFile>) {
  await pod.createDirectory(PROJECT_PATH, {recursive: true});

  const directories = new Set<string>();

  for (const file of files) {
    const directory = dirname(file.path);

    if (directory)
      directories.add(directory);
  }

  for (const directory of directories)
    await pod.createDirectory(`${PROJECT_PATH}/${directory}`, {recursive: true});

  for (const file of files) {
    const podFile = await pod.createFile(`${PROJECT_PATH}/${file.path}`, `text`);
    await podFile.write(file.content);
    await podFile.close();
  }
}

async function writeYarnBinary(pod: BrowserPodInstance) {
  const response = await fetch(YARN_BIN_ASSET);

  if (!response.ok)
    return false;

  const podFile = await pod.createFile(YARN_BIN_PATH, `binary`);
  await podFile.write(await response.arrayBuffer());
  await podFile.close();
  return true;
}

export function PlaygroundTerminal({files, version}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container)
      return undefined;

    let disposed = false;
    let term: XtermTerminal | null = null;
    let browserPodTerminal: BrowserPodTerminal | null = null;
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
        disableStdin: false,
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
      term.onData(data => browserPodTerminal?.readData(data));

      requestAnimationFrame(() => {
        if (!term || disposed)
          return;

        fitAddon.fit();
      });

      writeBootScreen(term, version);

      resizeObserver = new ResizeObserver(() => {
        if (!term || disposed)
          return;

        fitAddon.fit();
      });
      resizeObserver.observe(container);

      const apiKey = getBrowserPodApiKey();

      if (!apiKey) {
        writeLines(term, [
          `${yellow}[browserpod]${reset} Missing API key; set PUBLIC_BROWSERPOD_API_KEY or VITE_BP_APIKEY.`,
          `${dim}The terminal is wired to BrowserPod, but boot is skipped without credentials.${reset}`,
          `${green}$${reset} `,
        ]);
        return;
      }

      if (!(`SharedArrayBuffer` in window) || !Atomics.waitAsync) {
        writeLines(term, [
          `${red}[browserpod]${reset} BrowserPod requires SharedArrayBuffer and Atomics.waitAsync.`,
          `${dim}Serve this page with COOP/COEP headers and use a Chromium-based browser.${reset}`,
          `${green}$${reset} `,
        ]);
        return;
      }

      try {
        const {BrowserPod} = await import(/* @vite-ignore */ BROWSERPOD_RUNTIME_URL) as {BrowserPod: BrowserPodApi | null};

        if (!BrowserPod)
          throw new Error(`BrowserPod runtime failed to load`);

        writeLines(term, [`${cyan}[browserpod]${reset} Booting pod...`]);

        console.log(`Booting pod...`, apiKey);
        const pod = await BrowserPod.boot({apiKey});

        if (disposed || !term)
          return;

        browserPodTerminal = await pod.createCustomTerminal({
          cols: term.cols,
          rows: term.rows,
          onOutput: buffer => {
            if (!term || disposed)
              return;

            term.write(new Uint8Array(buffer));
          },
        });

        writeLines(term, [`${cyan}[browserpod]${reset} Syncing preset files...`]);
        await writeProjectFiles(pod, files);

        if (await writeYarnBinary(pod)) {
          writeLines(term, [
            `${cyan}[browserpod]${reset} Mounted ${YARN_BIN_ASSET}`,
          ]);

          await pod.run(`./yarn-bin`, [`--version`], {cwd: PROJECT_PATH, echo: true, terminal: browserPodTerminal});
          await pod.run(`./yarn-bin`, [`install`], {cwd: PROJECT_PATH, echo: true, terminal: browserPodTerminal});
        } else {
          writeLines(term, [
            `${yellow}[browserpod]${reset} ${YARN_BIN_ASSET} is missing.`,
            `${dim}Build it with: yarn workspace @yarnpkg/website build:browserpod-yarn${reset}`,
            `${cyan}[browserpod]${reset} Opening BrowserPod bash in the mounted project instead.`,
          ]);

          await pod.run(`bash`, [], {cwd: PROJECT_PATH, terminal: browserPodTerminal});
        }
      } catch (error) {
        if (!term || disposed)
          return;

        writeLines(term, [
          `${red}[browserpod]${reset} ${(error as Error).message}`,
          `${green}$${reset} `,
        ]);
      }
    }

    start();

    return () => {
      disposed = true;
      resizeObserver?.disconnect();
      term?.dispose();
    };
  }, [files, version]);

  return <div ref={containerRef} className={`playground-terminal-mount absolute inset-[20px_22px] min-h-0 min-w-0 rounded-xl border border-white/10 bg-black/85 shadow-[inset_0_1px_0_rgba(255,255,255,0.08)] max-[560px]:inset-3.5`} />;
}
