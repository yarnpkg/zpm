import {lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState} from 'react';

import type {PlaygroundEntry, PlaygroundTemplate}                          from '../../playground/types';
import {OctIcon}                                                           from '../package/icons';
import type {IconData}                                                     from '../package/types';

import {PlaygroundTerminal}                                                from './PlaygroundTerminal';
import type {PlaygroundFile, PlaygroundPodApi}                             from './PlaygroundTerminal';

const MonacoEditor = lazy(() => import(`@monaco-editor/react`).then(m => ({default: m.default})));

type TreeOcticons = {
  file: IconData;
  folder: IconData;
  terminal: IconData;
};

const TERMINAL_ENTRY: PlaygroundEntry = {depth: 0, name: `terminal`, path: `terminal`, kind: `terminal`};
const EMPTY_TEMPLATE: PlaygroundTemplate = {description: ``, entries: [], id: ``, label: `No templates`};

const EDIT_SYNC_DEBOUNCE_MS = 400;

const selectClassName = `h-[38px] w-full rounded-lg border border-[var(--line-strong)] bg-[color-mix(in_oklch,var(--fg)_6%,transparent)] px-3 font-mono text-xs font-medium text-[var(--fg)] outline-none focus:border-[var(--accent-line)] focus:shadow-[0_0_0_3px_var(--accent-soft)]`;
const treeItemClassName = `flex min-h-[30px] w-full items-center gap-2 whitespace-nowrap rounded-[7px] border-0 bg-transparent py-0 pr-2 text-left font-[inherit] text-[13px] leading-none text-[var(--fg-dim)] disabled:cursor-default enabled:cursor-pointer enabled:hover:bg-[color-mix(in_oklch,var(--fg)_7%,transparent)] enabled:hover:text-[var(--fg)]`;
const activeTreeItemClassName = `bg-[color-mix(in_oklch,var(--accent)_12%,transparent)] text-[var(--fg)]`;
const tabClassName = `inline-flex h-[38px] flex-none items-center gap-[7px] whitespace-nowrap border-x border-y-0 border-x-transparent bg-transparent px-3 font-mono text-xs font-medium text-[var(--fg-mute)] hover:bg-[color-mix(in_oklch,var(--fg)_5%,transparent)] hover:text-[var(--fg-dim)]`;
// Fixed width sized so `package.json` (the longest common name) fits without
// truncation; longer names get an ellipsis.
const fileTabClassName = `w-[164px]`;
const activeTabClassName = `border-x-[var(--line)] bg-[color-mix(in_oklch,var(--fg)_7%,transparent)] text-[var(--fg)]`;
const tabSelectClassName = `inline-flex min-w-0 flex-1 cursor-pointer items-center gap-[7px] border-0 bg-transparent p-0 font-[inherit] text-[length:inherit] text-inherit`;
const tabCloseClassName = `inline-flex h-4 w-4 flex-none cursor-pointer items-center justify-center rounded border-0 bg-transparent p-0 text-[13px] leading-none text-[var(--fg-mute)] hover:bg-[color-mix(in_oklch,var(--fg)_12%,transparent)] hover:text-[var(--fg)]`;

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
}

function requestedPresetId(templates: Array<PlaygroundTemplate>) {
  const requested = new URLSearchParams(window.location.search).get(`template`);
  return requested && templates.some(template => template.id === requested)
    ? requested
    : null;
}

function syncPresetToUrl(presetId: string) {
  if (typeof window === `undefined`)
    return;

  const url = new URL(window.location.href);
  url.searchParams.set(`template`, presetId);
  window.history.replaceState(null, ``, url);
}

export function PlaygroundWorkspace({version, octicons, templates}: {version: string, octicons: TreeOcticons, templates: Array<PlaygroundTemplate>}) {
  // This component renders with client:only, so the ?template= deep link can
  // be read synchronously here — the first render (and thus the single
  // BrowserPod boot) already uses the requested template.
  const [presetId, setPresetId] = useState(() => requestedPresetId(templates) ?? templates[0]?.id ?? ``);
  const [selectedPath, setSelectedPath] = useState(`terminal`);
  const [openFilePaths, setOpenFilePaths] = useState<Array<string>>([]);
  const [lastFilePath, setLastFilePath] = useState<string | null>(null);
  const [editedContents, setEditedContents] = useState<Record<string, string>>({});
  const [monacoReady, setMonacoReady] = useState(false);

  const podApiRef = useRef<PlaygroundPodApi | null>(null);
  const pendingWritesRef = useRef(new Map<string, string>());
  const syncTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const preset = templates.find(template => template.id === presetId) ?? templates[0] ?? EMPTY_TEMPLATE;
  const entries = preset.entries;

  useEffect(() => {
    if (templates.some(template => template.id === presetId))
      return;

    setPresetId(templates[0]?.id ?? ``);
  }, [presetId, templates]);

  const selectedEntry = useMemo(() => {
    if (selectedPath === `terminal`)
      return TERMINAL_ENTRY;

    return entries.find(file => file.path === selectedPath) ?? TERMINAL_ENTRY;
  }, [entries, selectedPath]);

  useEffect(() => {
    if (selectedPath === `terminal`)
      return;

    if (!entries.some(entry => entry.path === selectedPath && entry.kind !== `folder`)) {
      setSelectedPath(`terminal`);
    }
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

  const closeFile = useCallback((path: string) => {
    const remaining = openFilePaths.filter(other => other !== path);
    const fallback = remaining[remaining.length - 1] ?? null;

    setOpenFilePaths(remaining);

    if (selectedPath === path)
      setSelectedPath(fallback ?? `terminal`);
    if (lastFilePath === path) {
      setLastFilePath(fallback);
    }
  }, [lastFilePath, openFilePaths, selectedPath]);

  const flushPendingWrites = useCallback(() => {
    const api = podApiRef.current;
    if (!api)
      return;

    for (const [path, content] of pendingWritesRef.current) {
      api.writeFile(path, content).catch(() => {
        // Sync failures are non-fatal; the terminal reports pod-level errors.
      });
    }

    pendingWritesRef.current.clear();
  }, []);

  const handlePodApi = useCallback((api: PlaygroundPodApi | null) => {
    podApiRef.current = api;
    flushPendingWrites();
  }, [flushPendingWrites]);

  const handleEditorChange = useCallback((path: string, content: string) => {
    setEditedContents(contents => ({...contents, [path]: content}));
    pendingWritesRef.current.set(path, content);

    if (syncTimerRef.current !== null)
      clearTimeout(syncTimerRef.current);

    syncTimerRef.current = setTimeout(() => {
      syncTimerRef.current = null;
      flushPendingWrites();
    }, EDIT_SYNC_DEBOUNCE_MS);
  }, [flushPendingWrites]);

  useEffect(() => {
    return () => {
      if (syncTimerRef.current !== null) {
        clearTimeout(syncTimerRef.current);
      }
    };
  }, []);

  const handlePresetChange = useCallback((presetId: string) => {
    if (syncTimerRef.current !== null) {
      clearTimeout(syncTimerRef.current);
      syncTimerRef.current = null;
    }

    pendingWritesRef.current.clear();

    setPresetId(presetId);
    setOpenFilePaths([]);
    setLastFilePath(null);
    setEditedContents({});
    setSelectedPath(`terminal`);

    syncPresetToUrl(presetId);
  }, []);

  const handleMonacoMount = useCallback((_editor: any, monaco: any) => {
    setupPlaygroundMonacoTheme(monaco);
    setMonacoReady(true);
  }, []);

  // The playground window is pinned to the dark palette in both site themes,
  // so the editor always uses the dark theme.
  const editorTheme = monacoReady ? `playground-dark` : `vs-dark`;

  return (
    <div className={`grid min-h-0 grid-cols-[minmax(210px,18vw)_minmax(0,1fr)] max-[900px]:grid-cols-1 max-[900px]:grid-rows-[auto_minmax(0,1fr)]`}>
      <aside className={`min-w-0 overflow-auto border-r border-[var(--line)] bg-black/85 p-[18px] max-[900px]:max-h-[min(240px,32dvh)] max-[900px]:border-r-0 max-[900px]:border-b max-[560px]:p-3.5`} aria-label={`Playground files`}>
        <div className={`mb-[22px]`}>
          <select
            id={`playground-preset`}
            className={selectClassName}
            aria-label={`Playground preset`}
            value={presetId}
            onChange={event => handlePresetChange(event.currentTarget.value)}
          >
            {templates.map(template => (
              <option key={template.id} value={template.id}>
                {template.label}
              </option>
            ))}
          </select>
        </div>

        <div className={`mb-2 block font-mono text-[10px] uppercase tracking-[0.12em] text-[var(--fg-mute)]`}>
          Files
        </div>

        <ol className={`m-0 flex list-none flex-col gap-0.5 p-0`}>
          {entries.map(file => {
            const selectable = file.kind !== `folder`;
            const icon = file.kind === `terminal`
              ? octicons.terminal
              : file.kind === `folder`
                ? octicons.folder
                : octicons.file;

            return (
              <li key={file.path} className={`m-0 p-0`}>
                <button
                  type={`button`}
                  className={classNames(treeItemClassName, selectedPath === file.path && activeTreeItemClassName)}
                  style={{paddingLeft: 8 + file.depth * 16}}
                  disabled={!selectable}
                  aria-current={selectedPath === file.path ? `page` : undefined}
                  onClick={selectable ? () => selectEntry(file) : undefined}
                >
                  <span className={`inline-flex h-3.5 w-3.5 flex-none items-center justify-center text-[var(--fg-mute)]`} aria-hidden={`true`}>
                    {/* The folder glyph spans the full 16px grid while file
                        glyphs are inset; nudge it so left edges line up. */}
                    <OctIcon icon={icon} size={14} className={file.kind === `folder` ? `translate-x-[1.5px] scale-[0.92]` : undefined} />
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
            ? `bg-[radial-gradient(ellipse_70%_55%_at_75%_0%,oklch(0.55_0.10_205_/_0.08),transparent_70%),linear-gradient(180deg,rgba(0,0,0,0.96),rgba(0,0,0,0.92))] backdrop-blur-[20px] backdrop-saturate-150`
            : `bg-[radial-gradient(ellipse_70%_55%_at_75%_0%,oklch(0.62_0.12_195_/_0.08),transparent_70%),rgba(0,0,0,0.9)]`,
        )}
        aria-label={selectedEntry.kind === `terminal` ? `Terminal output` : `Editor`}
      >
        <div className={`flex min-w-0 items-end gap-0.5 overflow-x-auto border-b border-[var(--line)] bg-black/85 px-3 [scrollbar-width:thin]`} aria-label={`Open playground views`}>
          <button
            type={`button`}
            className={classNames(tabClassName, `cursor-pointer`, selectedPath === `terminal` && activeTabClassName)}
            onClick={() => setSelectedPath(`terminal`)}
          >
            <OctIcon icon={octicons.terminal} size={14} />
            <span className={`min-w-0 overflow-hidden text-ellipsis`}>terminal</span>
          </button>

          {openFileEntries.map(entry => (
            <div
              key={entry.path}
              className={classNames(tabClassName, fileTabClassName, selectedPath === entry.path && activeTabClassName)}
              title={entry.path}
            >
              <button
                type={`button`}
                className={tabSelectClassName}
                onClick={() => setSelectedPath(entry.path)}
              >
                <OctIcon icon={octicons.file} size={14} />
                <span className={`min-w-0 overflow-hidden text-ellipsis`}>{entry.name}</span>
              </button>
              <button
                type={`button`}
                className={tabCloseClassName}
                aria-label={`Close ${entry.name}`}
                onClick={() => closeFile(entry.path)}
              >
                ×
              </button>
            </div>
          ))}

        </div>

        <div className={`relative min-h-0 min-w-0`}>
          <div className={classNames(
            `absolute inset-0 min-h-0 min-w-0`,
            selectedEntry.kind === `terminal`
              ? `visible pointer-events-auto opacity-100`
              : `invisible pointer-events-none opacity-0`,
          )}>
            <PlaygroundTerminal files={terminalFiles} version={version} onApi={handlePodApi} />
          </div>

          {editorEntry && (
            <div className={classNames(
              `absolute inset-0 min-h-0 min-w-0`,
              selectedEntry.kind === `file`
                ? `visible pointer-events-auto opacity-100`
                : `invisible pointer-events-none opacity-0`,
            )}>
              <div className={`playground-editor-shell absolute inset-0 min-h-0 min-w-0`}>
                <Suspense fallback={<div className={`flex items-center p-[18px] font-mono text-xs text-[var(--fg-mute)]`}>Loading editor...</div>}>
                  <MonacoEditor
                    height={`100%`}
                    language={editorEntry.language ?? `plaintext`}
                    onChange={value => handleEditorChange(editorEntry.path, value ?? ``)}
                    onMount={handleMonacoMount}
                    options={{
                      automaticLayout: true,
                      contextmenu: false,
                      fontFamily: `'JetBrains Mono', ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace`,
                      fontSize: 13,
                      lineHeight: 20,
                      minimap: {enabled: false},
                      padding: {top: 16, bottom: 16},
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
                    value={editedContents[editorEntry.path] ?? editorEntry.content ?? ``}
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
