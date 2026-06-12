import {lazy, Suspense, useCallback, useEffect, useMemo, useState} from 'react';

import {OctIcon} from '../package/icons';
import {PlaygroundTerminal} from './PlaygroundTerminal';

import type {IconData} from '../package/types';
import type {PlaygroundFile} from './PlaygroundTerminal';

const MonacoEditor = lazy(() => import(`@monaco-editor/react`).then(m => ({default: m.default})));

type PlaygroundEntry = {
  content?: string;
  depth: number;
  kind: `file` | `folder` | `terminal`;
  language?: string;
  name: string;
  path: string;
};

type PresetId = `simple` | `workspaces` | `node-modules`;

type PlaygroundPreset = {
  entries: Array<PlaygroundEntry>;
  label: string;
};

type TreeOcticons = {
  file: IconData;
  folder: IconData;
  terminal: IconData;
};

const TERMINAL_ENTRY: PlaygroundEntry = {depth: 0, name: `terminal`, path: `terminal`, kind: `terminal`};

const SIMPLE_PROJECT: Array<PlaygroundEntry> = [
  {
    depth: 0,
    name: `package.json`,
    path: `package.json`,
    kind: `file`,
    language: `json`,
    content: `{
  "name": "simple-project",
  "packageManager": "yarn@6.0.0-git.20260507",
  "private": true,
  "scripts": {
    "start": "tsx src/index.ts"
  },
  "dependencies": {
    "lodash": "^4.17.21"
  },
  "devDependencies": {
    "tsx": "^5.0.0",
    "typescript": "^5.9.3"
  }
}
`,
  },
  {
    depth: 0,
    name: `.yarnrc.yml`,
    path: `.yarnrc.yml`,
    kind: `file`,
    language: `yaml`,
    content: `nodeLinker: pnp
enableGlobalCache: true
`,
  },
  {depth: 0, name: `src`, path: `src`, kind: `folder`},
  {
    depth: 1,
    name: `index.ts`,
    path: `src/index.ts`,
    kind: `file`,
    language: `typescript`,
    content: `import lodash from 'lodash';

const words = ['yarn', 'playground', 'wasm'];

console.log(lodash.startCase(words.join(' ')));
`,
  },
];

const WORKSPACES: Array<PlaygroundEntry> = [
  {
    depth: 0,
    name: `package.json`,
    path: `package.json`,
    kind: `file`,
    language: `json`,
    content: `{
  "name": "yarn-playground",
  "packageManager": "yarn@6.0.0-git.20260507",
  "private": true,
  "scripts": {
    "start": "tsx src/index.ts",
    "check": "yarn constraints"
  },
  "workspaces": [
    "packages/*"
  ],
  "dependencies": {
    "@yarnpkg/core": "workspace:*",
    "react": "^19.2.5"
  },
  "devDependencies": {
    "tsx": "^5.0.0",
    "typescript": "^5.9.3"
  }
}
`,
  },
  {
    depth: 0,
    name: `.yarnrc.yml`,
    path: `.yarnrc.yml`,
    kind: `file`,
    language: `yaml`,
    content: `nodeLinker: pnp
enableGlobalCache: true
enableImmutableInstalls: true

packageExtensions:
  "demo-plugin@*":
    peerDependencies:
      "@yarnpkg/core": "*"
`,
  },
  {depth: 0, name: `src`, path: `src`, kind: `folder`},
  {
    depth: 1,
    name: `index.ts`,
    path: `src/index.ts`,
    kind: `file`,
    language: `typescript`,
    content: `import {createWorkspace} from './workspace';

const workspace = createWorkspace({
  cwd: '/workspace',
  packageManager: process.env.npm_config_user_agent ?? 'yarn',
});

await workspace.install();
console.log(await workspace.explain('react'));
`,
  },
  {
    depth: 1,
    name: `workspace.ts`,
    path: `src/workspace.ts`,
    kind: `file`,
    language: `typescript`,
    content: `type WorkspaceOptions = {
  cwd: string;
  packageManager: string;
};

export function createWorkspace(options: WorkspaceOptions) {
  return {
    async install() {
      return {
        cwd: options.cwd,
        resolved: 42,
        linked: true,
      };
    },

    async explain(ident: string) {
      return \`\${ident} is provided by workspace:demo\`;
    },
  };
}
`,
  },
  {depth: 0, name: `packages`, path: `packages`, kind: `folder`},
  {depth: 1, name: `app`, path: `packages/app`, kind: `folder`},
  {
    depth: 2,
    name: `package.json`,
    path: `packages/app/package.json`,
    kind: `file`,
    language: `json`,
    content: `{
  "name": "@demo/app",
  "private": true,
  "dependencies": {
    "@demo/tools": "workspace:*",
    "react": "^19.2.5"
  }
}
`,
  },
  {depth: 1, name: `tools`, path: `packages/tools`, kind: `folder`},
  {
    depth: 2,
    name: `package.json`,
    path: `packages/tools/package.json`,
    kind: `file`,
    language: `json`,
    content: `{
  "name": "@demo/tools",
  "private": true,
  "exports": {
    ".": "./src/index.ts"
  }
}
`,
  },
];

const NODE_MODULES_LINKER: Array<PlaygroundEntry> = [
  {
    depth: 0,
    name: `package.json`,
    path: `package.json`,
    kind: `file`,
    language: `json`,
    content: `{
  "name": "node-modules-linker",
  "packageManager": "yarn@6.0.0-git.20260507",
  "private": true,
  "scripts": {
    "test": "vitest run"
  },
  "dependencies": {
    "express": "^5.2.1"
  },
  "devDependencies": {
    "vitest": "^4.0.0"
  }
}
`,
  },
  {
    depth: 0,
    name: `.yarnrc.yml`,
    path: `.yarnrc.yml`,
    kind: `file`,
    language: `yaml`,
    content: `nodeLinker: node-modules
nmMode: hardlinks-global
enableGlobalCache: true
`,
  },
  {depth: 0, name: `src`, path: `src`, kind: `folder`},
  {
    depth: 1,
    name: `server.ts`,
    path: `src/server.ts`,
    kind: `file`,
    language: `typescript`,
    content: `import express from 'express';

const app = express();

app.get('/', (_req, res) => {
  res.send('Hello from Yarn with node_modules');
});

app.listen(3000);
`,
  },
  {depth: 0, name: `node_modules`, path: `node_modules`, kind: `folder`},
  {depth: 1, name: `.yarn-state.yml`, path: `node_modules/.yarn-state.yml`, kind: `file`, language: `yaml`, content: `# Generated by Yarn
__metadata:
  version: 6
  nmMode: hardlinks-global
`},
];

const PRESETS: Record<PresetId, PlaygroundPreset> = {
  simple: {
    label: `Simple project`,
    entries: SIMPLE_PROJECT,
  },
  workspaces: {
    label: `Workspaces`,
    entries: WORKSPACES,
  },
  'node-modules': {
    label: `Node modules linker`,
    entries: NODE_MODULES_LINKER,
  },
};

const selectClassName = `h-[38px] w-full rounded-lg border border-[var(--line-strong)] bg-[color-mix(in_oklch,var(--fg)_6%,transparent)] px-3 font-mono text-xs font-medium text-[var(--fg)] outline-none focus:border-[var(--accent-line)] focus:shadow-[0_0_0_3px_var(--accent-soft)]`;
const treeItemClassName = `flex min-h-[30px] w-full items-center gap-2 whitespace-nowrap rounded-[7px] border-0 bg-transparent py-0 pr-2 text-left font-[inherit] text-[13px] leading-none text-[var(--fg-dim)] disabled:cursor-default enabled:cursor-pointer enabled:hover:bg-[color-mix(in_oklch,var(--fg)_7%,transparent)] enabled:hover:text-[var(--fg)]`;
const activeTreeItemClassName = `bg-[color-mix(in_oklch,var(--accent)_12%,transparent)] text-[var(--fg)]`;
const tabClassName = `inline-flex h-[38px] max-w-[220px] flex-none cursor-pointer items-center gap-[7px] whitespace-nowrap border-x border-y-0 border-x-transparent bg-transparent px-3 font-mono text-xs font-medium text-[var(--fg-mute)] hover:bg-[color-mix(in_oklch,var(--fg)_5%,transparent)] hover:text-[var(--fg-dim)]`;
const activeTabClassName = `border-x-[var(--line)] bg-[color-mix(in_oklch,var(--fg)_7%,transparent)] text-[var(--fg)]`;

function classNames(...classes: Array<string | false | null | undefined>) {
  return classes.filter(Boolean).join(` `);
}

function setupPlaygroundMonacoTheme(monaco: any) {
  monaco.editor.defineTheme(`playground-dark`, {
    base: `vs-dark`,
    inherit: true,
    rules: [
      {token: `comment`, foreground: `6872a0`},
      {token: `keyword`, foreground: `c4a0f5`},
      {token: `string`, foreground: `a0dbb0`},
      {token: `number`, foreground: `d4c080`},
      {token: `type`, foreground: `90c8e8`},
      {token: `function`, foreground: `90c8e8`},
    ],
    colors: {
      'editor.background': `#00000000`,
      'editor.foreground': `#d6daf5`,
      'editor.lineHighlightBackground': `#ffffff08`,
      'editorLineNumber.foreground': `#6872a0`,
      'editorLineNumber.activeForeground': `#a8b0d4`,
      'editor.selectionBackground': `#7dd3fc44`,
      'editor.inactiveSelectionBackground': `#ffffff0d`,
      'editorIndentGuide.background': `#ffffff0a`,
      'editorIndentGuide.activeBackground': `#ffffff18`,
      'scrollbarSlider.background': `#a8b0d428`,
      'scrollbarSlider.hoverBackground': `#a8b0d440`,
    },
  });

  monaco.editor.defineTheme(`playground-light`, {
    base: `vs`,
    inherit: true,
    rules: [
      {token: `comment`, foreground: `7a84a8`},
      {token: `keyword`, foreground: `7030b0`},
      {token: `string`, foreground: `286840`},
      {token: `number`, foreground: `885510`},
      {token: `type`, foreground: `205878`},
      {token: `function`, foreground: `205878`},
    ],
    colors: {
      'editor.background': `#00000000`,
      'editor.foreground': `#0c1030`,
      'editor.lineHighlightBackground': `#00000005`,
      'editorLineNumber.foreground': `#515a7a`,
      'editorLineNumber.activeForeground': `#252d50`,
      'editor.selectionBackground': `#0c103018`,
      'editor.inactiveSelectionBackground': `#0c10300d`,
      'editorIndentGuide.background': `#0c103010`,
      'editorIndentGuide.activeBackground': `#0c103020`,
      'scrollbarSlider.background': `#0c103018`,
      'scrollbarSlider.hoverBackground': `#0c103028`,
    },
  });
}

export function PlaygroundWorkspace({version, octicons}: {version: string, octicons: TreeOcticons}) {
  const [presetId, setPresetId] = useState<PresetId>(`simple`);
  const [selectedPath, setSelectedPath] = useState(`terminal`);
  const [openFilePaths, setOpenFilePaths] = useState<Array<string>>([]);
  const [lastFilePath, setLastFilePath] = useState<string | null>(null);
  const [monacoReady, setMonacoReady] = useState(false);
  const [isDark, setIsDark] = useState(() => typeof document !== `undefined` && document.documentElement.getAttribute(`data-theme`) !== `light`);

  const preset = PRESETS[presetId];
  const entries = preset.entries;

  const selectedEntry = useMemo(() => {
    if (selectedPath === `terminal`)
      return TERMINAL_ENTRY;

    return entries.find(file => file.path === selectedPath) ?? entries[0];
  }, [entries, selectedPath]);

  useEffect(() => {
    if (selectedPath === `terminal`)
      return;

    if (!entries.some(entry => entry.path === selectedPath && entry.kind !== `folder`))
      setSelectedPath(`terminal`);
  }, [entries, selectedPath]);

  const openFileEntries = useMemo(() => {
    return openFilePaths
      .map(path => entries.find(entry => entry.path === path && entry.kind === `file`))
      .filter((entry): entry is PlaygroundEntry => !!entry);
  }, [entries, openFilePaths]);

  const terminalFiles = useMemo<Array<PlaygroundFile>>(() => {
    return entries
      .filter((entry): entry is PlaygroundEntry & {content: string} => entry.kind === `file` && typeof entry.content === `string`)
      .map(entry => ({path: entry.path, content: entry.content}));
  }, [entries]);

  const editorEntry = useMemo(() => {
    if (selectedEntry.kind === `file`)
      return selectedEntry;

    return openFileEntries.find(entry => entry.path === lastFilePath) ?? openFileEntries[0] ?? null;
  }, [lastFilePath, openFileEntries, selectedEntry]);

  const selectEntry = useCallback((entry: PlaygroundEntry) => {
    if (entry.kind === `folder`)
      return;

    if (entry.kind === `file`) {
      setOpenFilePaths(paths => paths.includes(entry.path) ? paths : [...paths, entry.path]);
      setLastFilePath(entry.path);
    }

    setSelectedPath(entry.path);
  }, []);

  const handlePresetChange = useCallback((presetId: PresetId) => {
    setPresetId(presetId);
    setOpenFilePaths([]);
    setLastFilePath(null);
    setSelectedPath(`terminal`);
  }, []);

  const handleMonacoMount = useCallback((_editor: any, monaco: any) => {
    setupPlaygroundMonacoTheme(monaco);
    setMonacoReady(true);
  }, []);

  useEffect(() => {
    const handler = (event: Event) => setIsDark((event as CustomEvent).detail !== `light`);
    window.addEventListener(`themechange`, handler);
    return () => window.removeEventListener(`themechange`, handler);
  }, []);

  const editorTheme = monacoReady
    ? isDark ? `playground-dark` : `playground-light`
    : isDark ? `vs-dark` : `vs`;

  return (
    <div className="grid min-h-0 grid-cols-[minmax(210px,18vw)_minmax(0,1fr)] max-[900px]:grid-cols-1 max-[900px]:grid-rows-[auto_minmax(0,1fr)]">
      <aside className="min-w-0 overflow-auto border-r border-[var(--line)] bg-[rgba(3,6,16,0.78)] p-[18px] max-[900px]:max-h-[min(240px,32dvh)] max-[900px]:border-r-0 max-[900px]:border-b max-[560px]:p-3.5" aria-label="Playground files">
        <select id="playground-version" className={`${selectClassName} mb-4`} aria-label="Yarn version" defaultValue={`Yarn ${version}`}>
          <option>{`Yarn ${version}`}</option>
          <option>Yarn stable</option>
          <option>Yarn canary</option>
        </select>

        <div className="mb-[22px]">
          <select
            id="playground-preset"
            className={selectClassName}
            aria-label="Playground preset"
            value={presetId}
            onChange={event => handlePresetChange(event.currentTarget.value as PresetId)}
          >
            {Object.entries(PRESETS).map(([id, preset]) => (
              <option key={id} value={id}>
                {preset.label}
              </option>
            ))}
          </select>
        </div>

        <div className="mb-2 block font-mono text-[10px] uppercase tracking-[0.12em] text-[var(--fg-mute)]">
          Files
        </div>

        <ol className="m-0 flex list-none flex-col gap-0.5 p-0">
          {entries.map(file => {
            const selectable = file.kind !== `folder`;
            const icon = file.kind === `terminal`
              ? octicons.terminal
              : file.kind === `folder`
                ? octicons.folder
                : octicons.file;

            return (
              <li key={file.path} className="m-0 p-0">
                <button
                  type="button"
                  className={classNames(treeItemClassName, selectedPath === file.path && activeTreeItemClassName)}
                  style={{paddingLeft: 8 + file.depth * 16}}
                  disabled={!selectable}
                  aria-current={selectedPath === file.path ? `page` : undefined}
                  onClick={selectable ? () => selectEntry(file) : undefined}
                >
                  <span className="inline-flex h-3.5 w-3.5 flex-none items-center justify-center text-[var(--fg-mute)]" aria-hidden="true">
                    <OctIcon icon={icon} size={14} />
                  </span>
                  <span>{file.name}</span>
                </button>
              </li>
            );
          })}
        </ol>
      </aside>

      <div
        className={classNames(
          `relative grid min-h-0 min-w-0 grid-rows-[auto_minmax(0,1fr)]`,
          selectedEntry.kind === `terminal`
            ? `bg-[radial-gradient(ellipse_70%_55%_at_75%_0%,oklch(0.55_0.10_205_/_0.10),transparent_70%),linear-gradient(180deg,rgba(2,5,14,0.92),rgba(2,3,10,0.84))] backdrop-blur-[20px] backdrop-saturate-150`
            : `bg-[radial-gradient(ellipse_70%_55%_at_75%_0%,oklch(0.62_0.12_195_/_0.10),transparent_70%),rgba(2,4,12,0.78)]`,
        )}
        aria-label={selectedEntry.kind === `terminal` ? `Terminal output` : `Editor`}
      >
        <div className="flex min-w-0 items-end gap-0.5 overflow-x-auto border-b border-[var(--line)] bg-[rgba(3,6,16,0.72)] px-3 [scrollbar-width:thin]" role="tablist" aria-label="Open playground views">
          <button
            type="button"
            className={classNames(tabClassName, selectedPath === `terminal` && activeTabClassName)}
            role="tab"
            aria-selected={selectedPath === `terminal`}
            onClick={() => setSelectedPath(`terminal`)}
          >
            <OctIcon icon={octicons.terminal} size={14} />
            <span className="min-w-0 overflow-hidden text-ellipsis">terminal</span>
          </button>

          {openFileEntries.map(entry => (
            <button
              key={entry.path}
              type="button"
              className={classNames(tabClassName, selectedPath === entry.path && activeTabClassName)}
              role="tab"
              aria-selected={selectedPath === entry.path}
              title={entry.path}
              onClick={() => setSelectedPath(entry.path)}
            >
              <OctIcon icon={octicons.file} size={14} />
              <span className="min-w-0 overflow-hidden text-ellipsis">{entry.name}</span>
            </button>
          ))}
        </div>

        <div className="relative min-h-0 min-w-0">
          <div className={classNames(`invisible pointer-events-none absolute inset-0 min-h-0 min-w-0 opacity-0`, selectedEntry.kind === `terminal` && `visible pointer-events-auto opacity-100`)}>
            <PlaygroundTerminal files={terminalFiles} version={version} />
          </div>

          {editorEntry && (
            <div className={classNames(`invisible pointer-events-none absolute inset-0 min-h-0 min-w-0 opacity-0`, selectedEntry.kind === `file` && `visible pointer-events-auto opacity-100`)}>
              <div className="playground-editor-shell absolute inset-0 min-h-0 min-w-0">
                <Suspense fallback={<div className="flex items-center p-[18px] font-mono text-xs text-[var(--fg-mute)]">Loading editor...</div>}>
                  <MonacoEditor
                    height="100%"
                    language={editorEntry.language ?? `plaintext`}
                    onMount={handleMonacoMount}
                    options={{
                      automaticLayout: true,
                      contextmenu: false,
                      fontFamily: `'JetBrains Mono', ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace`,
                      fontSize: 13,
                      lineHeight: 20,
                      minimap: {enabled: false},
                      padding: {top: 16, bottom: 16},
                      readOnly: true,
                      renderLineHighlight: `line`,
                      scrollBeyondLastLine: false,
                      scrollbar: {
                        horizontalScrollbarSize: 8,
                        verticalScrollbarSize: 8,
                      },
                      wordWrap: `on`,
                    }}
                    path={editorEntry.path}
                    theme={editorTheme}
                    value={editorEntry.content ?? ``}
                  />
                </Suspense>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
