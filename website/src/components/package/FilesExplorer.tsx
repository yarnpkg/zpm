import {useState, useEffect, useCallback, useRef, useMemo, lazy, Suspense}                                                                          from 'react';

import {useIcons}                                                                                                                                   from './contexts';
import {OctIcon}                                                                                                                                    from './icons';
import type {FileEntry, Tab, TreeNode}                                                                                                              from './types';
import {formatBytes, timeAgo, langFromPath, setupMonacoTheme, buildFileTree, formatWithPrettier, canPrettify, compareSemverDesc, isNoisyPrerelease} from './utils';

const MonacoEditor = lazy(() => import(`@monaco-editor/react`).then(m => ({default: m.default})));
const MonacoDiffEditor = lazy(() => import(`@monaco-editor/react`).then(m => ({default: m.DiffEditor})));

// ── Internal components ──

function SmallChevron({open}: {open: boolean}) {
  const {oct} = useIcons();
  return <OctIcon icon={oct[`chevron-right`]} size={12} className={`transition-transform duration-[120ms] ${open ? `rotate-90` : ``}`}/>;
}

function TreeFileIcon() {
  const {oct} = useIcons();
  return <OctIcon icon={oct[`file-code`]} size={14}/>;
}

function TreeFolderIcon({open}: {open: boolean}) {
  const {oct} = useIcons();
  return <OctIcon icon={oct[open ? `file-directory-open-fill` : `file-directory-fill`]} size={14}/>;
}

function CompareIcon() {
  const {oct} = useIcons();
  return <OctIcon icon={oct.diff} size={14}/>;
}

const CHANGE_COLORS: Record<string, string> = {
  added: `oklch(0.75 0.15 145)`,
  removed: `oklch(0.75 0.15 25)`,
  modified: `oklch(0.80 0.14 80)`,
};

function ExplorerTreeNode({node, depth, selectedFile, onSelectFile, changeMap}: {
  node: TreeNode; depth: number; selectedFile: string | null; onSelectFile: (path: string) => void;
  changeMap?: Map<string, string>;
}) {
  const [expanded, setExpanded] = useState(depth < 1);

  const isDir = !!node.children;
  const indent = depth * 16;

  const changeType = changeMap?.get(node.path);
  const nameColor = changeType ? CHANGE_COLORS[changeType] : undefined;

  if (isDir) {
    return (
      <>
        <div
          className={`ftree-row`}
          style={{paddingLeft: `${indent + 4}px`}}
          onClick={() => setExpanded(!expanded)}
        >
          <span className={`ftree-chevron`}><SmallChevron open={expanded}/></span>
          <span className={`ftree-icon`}><TreeFolderIcon open={expanded}/></span>
          <span className={`ftree-name`}>{node.name}</span>
        </div>
        {expanded && node.children?.map(child => (
          <ExplorerTreeNode key={child.path} node={child} depth={depth + 1} selectedFile={selectedFile} onSelectFile={onSelectFile} changeMap={changeMap}/>
        ))}
      </>
    );
  }

  return (
    <div
      className={`ftree-row${selectedFile === node.path ? ` active` : ``}`}
      style={{paddingLeft: `${indent + 4}px`}}
      onClick={() => onSelectFile(node.path)}
    >
      <span className={`ftree-chevron invisible`}><SmallChevron open={false}/></span>
      <span className={`ftree-icon`}><TreeFileIcon/></span>
      <span className={`ftree-name`} style={nameColor ? {color: nameColor} : undefined}>{node.name}</span>
      {node.size != null && <span className={`ftree-size`}>{formatBytes(node.size)}</span>}
    </div>
  );
}

// ── Main component ──

export function FilesExplorer({
  files, name, version, versions, distTags, time, onVersionChange,
  onTabChange, onFileChange, onCompareChange, selectedFile, compareVersion,
}: {
  files: Array<FileEntry> | null;
  name: string; version: string;
  versions: Array<string>;
  distTags: Record<string, string>;
  time: Record<string, string>;
  onVersionChange: (v: string) => void;
  onTabChange: (t: Tab) => void;
  onFileChange: (path: string | null) => void;
  onCompareChange: (compareVersion: string | null) => void;
  selectedFile: string | null;
  compareVersion: string | null;
}) {
  const {oct} = useIcons();

  const [fileContent, setFileContent] = useState<string | null>(null);
  const [fileLoading, setFileLoading] = useState(false);
  const [fileError, setFileError] = useState<string | null>(null);
  const [monacoReady, setMonacoReady] = useState(false);
  const [explorerOpen, setExplorerOpen] = useState(true);
  const [versionsOpen, setVersionsOpen] = useState(true);
  const [formatOpen, setFormatOpen] = useState(true);
  const monacoRef = useRef<any>(null);

  const [compareFiles, setCompareFiles] = useState<Array<FileEntry> | null>(null);
  const [origContent, setOrigContent] = useState<string | null>(null);
  const [origLoading, setOrigLoading] = useState(false);

  const [prettify, setPrettify] = useState(false);
  const [formattedContent, setFormattedContent] = useState<string | null>(null);
  const [formattedOrig, setFormattedOrig] = useState<string | null>(null);

  const sorted = useMemo(() =>
    versions.filter(v => !isNoisyPrerelease(v)).sort(compareSemverDesc)
  , [versions]);

  const tagForVersion = (v: string) => Object.entries(distTags).find(([, ver]) => ver === v)?.[0];

  const exitCompare = useCallback(() => {
    onCompareChange(null);
  }, [onCompareChange]);

  useEffect(() => {
    if (!compareVersion || !name) {
      setCompareFiles(null);
      return undefined;
    }
    const abortCtrl = new AbortController();
    fetch(`https://data.jsdelivr.com/v1/package/npm/${name}@${compareVersion}/flat`, {signal: abortCtrl.signal})
      .then(r => r.ok ? r.json() : null)
      .then(data => {
        if (data?.files) {
          setCompareFiles(data.files);
        }
      })
      .catch(err => {
        if (err.name !== `AbortError`) {
          setCompareFiles(null);
        }
      });
    return () => abortCtrl.abort();
  }, [compareVersion, name]);

  const changeMap = useMemo(() => {
    if (!compareVersion || !files || !compareFiles)
      return null;

    const map = new Map<string, string>();
    const oldByPath = new Map(compareFiles.map(f => [f.name.replace(/^\//, ``), f.hash]));
    const newByPath = new Map(files.map(f => [f.name.replace(/^\//, ``), f.hash]));

    for (const [path, hash] of newByPath) {
      const oldHash = oldByPath.get(path);
      if (!oldHash) {
        map.set(path, `added`);
      } else if (oldHash !== hash) {
        map.set(path, `modified`);
      }
    }
    for (const [path] of oldByPath) {
      if (!newByPath.has(path)) {
        map.set(path, `removed`);
      }
    }

    return map;
  }, [compareVersion, files, compareFiles]);

  const displayFiles = useMemo(() => {
    if (!files)
      return null;

    if (!changeMap)
      return files;

    const removedFiles: Array<FileEntry> = compareFiles
      ? compareFiles.filter(f => changeMap.get(f.name.replace(/^\//, ``)) === `removed`)
      : [];
    const currentChanged = files.filter(f => changeMap.has(f.name.replace(/^\//, ``)));
    return [...currentChanged, ...removedFiles];
  }, [files, compareFiles, changeMap]);

  const tree = displayFiles ? buildFileTree(displayFiles, name) : null;

  useEffect(() => {
    if (!selectedFile || !name || !version) {
      setFileContent(null);
      setFileError(null);
      return undefined;
    }
    const changeType = changeMap?.get(selectedFile);
    if (changeType === `removed`) {
      setFileContent(``);
      setFileLoading(false);
      setFileError(null);
      return undefined;
    }
    const abortCtrl = new AbortController();
    setFileLoading(true);
    setFileContent(null);
    setFileError(null);
    fetch(`https://cdn.jsdelivr.net/npm/${name}@${version}/${selectedFile}`, {signal: abortCtrl.signal})
      .then(r => {
        if (!r.ok) throw new Error(`Failed to load file`);
        return r.text();
      })
      .then(text => setFileContent(text))
      .catch(err => {
        if (err.name !== `AbortError`) {
          setFileContent(null);
          setFileError(err.message);
        }
      })
      .finally(() => setFileLoading(false));
    return () => abortCtrl.abort();
  }, [selectedFile, name, version, changeMap]);

  useEffect(() => {
    if (!compareVersion || !selectedFile || !name) {
      setOrigContent(null);
      return undefined;
    }
    const changeType = changeMap?.get(selectedFile);
    if (changeType === `added`) {
      setOrigContent(``);
      return undefined;
    }
    const abortCtrl = new AbortController();
    setOrigLoading(true);
    setOrigContent(null);
    fetch(`https://cdn.jsdelivr.net/npm/${name}@${compareVersion}/${selectedFile}`, {signal: abortCtrl.signal})
      .then(r => {
        if (!r.ok) throw new Error(`fetch failed`);
        return r.text();
      })
      .then(text => setOrigContent(text))
      .catch(err => {
        if (err.name !== `AbortError`) {
          setOrigContent(``);
        }
      })
      .finally(() => setOrigLoading(false));
    return () => abortCtrl.abort();
  }, [compareVersion, selectedFile, name, changeMap]);

  useEffect(() => {
    if (!prettify || fileContent == null || !selectedFile) {
      setFormattedContent(null);
      return undefined;
    }
    let cancelled = false;
    formatWithPrettier(fileContent, selectedFile)
      .then(result => {
        if (!cancelled) {
          setFormattedContent(result);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setFormattedContent(null);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [prettify, fileContent, selectedFile]);

  useEffect(() => {
    if (!prettify || origContent == null || !selectedFile) {
      setFormattedOrig(null);
      return undefined;
    }
    let cancelled = false;
    formatWithPrettier(origContent, selectedFile)
      .then(result => {
        if (!cancelled) {
          setFormattedOrig(result);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setFormattedOrig(null);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [prettify, origContent, selectedFile]);

  const prevVersionRef = useRef(version);
  useEffect(() => {
    if (prevVersionRef.current && prevVersionRef.current !== version) {
      setFileContent(null);
      setCompareFiles(null);
      setOrigContent(null);
      setFileLoading(false);
      setOrigLoading(false);
      setFileError(null);
    }
    prevVersionRef.current = version;
  }, [version]);

  const handleMonacoMount = useCallback((_editor: any, monaco: any) => {
    monacoRef.current = monaco;
    setupMonacoTheme(monaco);
    setMonacoReady(true);
  }, []);

  const [isDark, setIsDark] = useState(() => typeof document !== `undefined` && document.documentElement.getAttribute(`data-theme`) !== `light`);
  useEffect(() => {
    const handler = (e: Event) => setIsDark((e as CustomEvent).detail !== `light`);
    window.addEventListener(`themechange`, handler);
    return () => window.removeEventListener(`themechange`, handler);
  }, []);
  const editorTheme = monacoReady ? (isDark ? `pkg-dark` : `pkg-light`) : (isDark ? `vs-dark` : `vs`);

  const inCompare = !!compareVersion;

  const awaitingFormat = prettify && selectedFile != null && canPrettify(selectedFile);
  const contentFormatted = !awaitingFormat || formattedContent != null;
  const origFormatted = !awaitingFormat || !inCompare || formattedOrig != null;

  const isLoading = fileLoading || (inCompare && origLoading) || (awaitingFormat && (!contentFormatted || !origFormatted));

  const displayContent = awaitingFormat && formattedContent != null ? formattedContent : fileContent;
  const displayOrig = awaitingFormat && formattedOrig != null ? formattedOrig : origContent;

  const topBar = (
    <div className={`flex items-center border-b border-[var(--line-strong)] bg-[var(--card)] backdrop-blur-lg px-4 shrink-0 h-10`}>
      <button
        onClick={() => onTabChange(`readme`)}
        className={`inline-flex items-center gap-1.5 font-sans text-[12.5px] text-[var(--fg-mute)] bg-transparent border-0 cursor-pointer transition-colors hover:text-[var(--fg)] mr-6`}
      >
        ← Back to the package page
      </button>

      <div className={`flex items-center gap-2`}>
        <span className={`mono text-[13px] font-medium text-[var(--fg)]`}>{name}</span>
        {inCompare ? (
          <span className={`inline-flex items-center gap-1.5 mono text-[11px] py-0.5 px-2 rounded-full bg-[var(--accent-soft)] border border-[var(--accent-line)] text-[var(--accent)]`}>
            {version}
            <span className={`text-[var(--fg-mute)]`}>←</span>
            {compareVersion}
            <button
              onClick={exitCompare}
              className={`inline-flex items-center justify-center w-4 h-4 rounded-full bg-transparent border-0 text-inherit p-0 ml-0.5 hover:bg-[color-mix(in_oklch,var(--accent)_20%,transparent)] transition-colors cursor-pointer`}
            >
              <OctIcon icon={oct.x} size={8}/>
            </button>
          </span>
        ) : (
          <span className={`mono text-[11px] text-[var(--fg-mute)]`}>{version}</span>
        )}
      </div>
    </div>
  );

  const changedCount = changeMap?.size ?? 0;
  const filesLoading = !files;

  return (
    <div className={`flex flex-col h-[calc(100vh-67px)]`}>
      {topBar}

      <div className={`files-explorer`}>
        {/* Sidebar */}
        <div className={`files-sidebar`}>
          <div className={`files-sidebar-section`} onClick={() => setExplorerOpen(!explorerOpen)}>
            <SmallChevron open={explorerOpen}/>
            {inCompare ? `Changed files` : `Explorer`}
            <span className={`ml-auto text-[10px] tracking-[0.06em]`}>
              {filesLoading ? `…` : inCompare ? (changeMap ? changedCount : `…`) : `${files.length} files`}
            </span>
          </div>
          {explorerOpen && (
            <div className={`files-tree-wrap`}>
              {filesLoading ? (
                <div className={`flex items-center justify-center py-6`}>
                  <div className={`w-5 h-5 border-2 border-[var(--line-strong)] border-t-[var(--accent)] rounded-full`} style={{animation: `pkg-spin 0.6s linear infinite`}}/>
                </div>
              ) : inCompare && !compareFiles ? (
                <div className={`flex items-center justify-center py-6`}>
                  <div className={`w-5 h-5 border-2 border-[var(--line-strong)] border-t-[var(--accent)] rounded-full`} style={{animation: `pkg-spin 0.6s linear infinite`}}/>
                </div>
              ) : tree && tree.children?.map(child => (
                <ExplorerTreeNode key={child.path} node={child} depth={0} selectedFile={selectedFile} onSelectFile={onFileChange} changeMap={changeMap ?? undefined}/>
              ))}
            </div>
          )}

          <div className={`files-sidebar-section`} onClick={() => setVersionsOpen(!versionsOpen)}>
            <SmallChevron open={versionsOpen}/>
            Versions
            <span className={`ml-auto text-[10px] tracking-[0.06em]`}>{versions.length}</span>
          </div>
          {versionsOpen && (
            <div className={`files-versions-wrap`}>
              {sorted.map(v => {
                const tag = tagForVersion(v);
                const isCompareTarget = v === compareVersion;
                return (
                  <div
                    key={v}
                    className={`fver-row${v === version ? ` active` : ``}${isCompareTarget ? ` comparing` : ``}`}
                    onClick={() => onVersionChange(v)}
                  >
                    <span>{v}</span>
                    {tag && (
                      <span className={`font-sans text-[9px] tracking-[0.1em] uppercase py-0.5 px-1 rounded ${
                        tag === `latest` ? `bg-[var(--accent-soft)] text-[var(--accent)]` : `bg-[color-mix(in_oklch,var(--fg)_8%,transparent)] text-[var(--fg-mute)]`
                      }`}>{tag}</span>
                    )}
                    <span className={`flex-1`}/>
                    {v !== version && (
                      <button
                        className={`fver-compare-btn${isCompareTarget ? ` active` : ``}`}
                        onClick={e => {
                          e.stopPropagation();
                          if (isCompareTarget) {
                            exitCompare();
                          } else {
                            onCompareChange(v);
                          }
                        }}
                      >
                        {isCompareTarget ? `Comparing` : `Compare?`}
                      </button>
                    )}
                    <span className={`text-[10px] text-[var(--fg-mute)]`}>{time[v] ? timeAgo(time[v]) : ``}</span>
                  </div>
                );
              })}
            </div>
          )}

          <div className={`files-sidebar-section`} onClick={() => setFormatOpen(!formatOpen)}>
            <SmallChevron open={formatOpen}/>
            Format
          </div>
          {formatOpen && (
            <div className={`pt-1.5 px-3.5 pb-2.5`}>
              <div
                className={`flex items-center gap-[6px] cursor-pointer text-[13px] text-[var(--fg-dim)] select-none`}
                onClick={() => setPrettify(!prettify)}
              >
                <span className={`inline-flex items-center justify-center w-[14px] h-[14px] rounded border text-[9px] leading-none flex-shrink-0 ${
                  prettify
                    ? `border-[var(--accent)] bg-[var(--accent)] text-[var(--bg-0)]`
                    : `border-[var(--fg-mute)] bg-transparent text-transparent`
                }`}>
                  {prettify && `\u2713`}
                </span>
                Prettify
                {prettify && selectedFile && !canPrettify(selectedFile) && (
                  <span className={`ml-auto text-[9px] text-[var(--fg-mute)]`}>unsupported</span>
                )}
              </div>
            </div>
          )}
        </div>

        {/* Editor */}
        <div className={`files-editor`}>
          {selectedFile ? (
            <>
              <div className={`files-editor-tab`}>
                <span className={`ftree-icon`}><TreeFileIcon/></span>
                <span className={`text-[var(--fg)]`} style={changeMap?.get(selectedFile) ? {color: CHANGE_COLORS[changeMap.get(selectedFile)!]} : undefined}>{selectedFile}</span>
                {changeMap?.get(selectedFile) && (
                  <span className={`mono text-[10px] uppercase tracking-[0.08em]`} style={{color: CHANGE_COLORS[changeMap.get(selectedFile)!]}}>{changeMap.get(selectedFile)}</span>
                )}
                {!inCompare && files && (() => {
                  const f = files.find(f => f.name === selectedFile || f.name === `/${selectedFile}`);
                  return f ? <span className={`text-[10.5px] text-[var(--fg-mute)] ml-auto`}>{formatBytes(f.size)}</span> : null;
                })()}
              </div>
              <div className={`files-editor-body`}>
                {isLoading ? (
                  <div className={`files-editor-empty`}>
                    <div className={`w-6 h-6 border-2 border-[var(--line-strong)] border-t-[var(--accent)] rounded-full`} style={{animation: `pkg-spin 0.6s linear infinite`}}/>
                    <span>Loading…</span>
                  </div>
                ) : fileError ? (
                  <div className={`files-editor-empty`}>
                    <span>Could not load file</span>
                    <span className={`text-[11px] text-[var(--fg-mute)]`}>{fileError}</span>
                  </div>
                ) : inCompare && displayOrig != null && displayContent != null ? (
                  <Suspense fallback={<div className={`files-editor-empty`}><span>Loading diff editor…</span></div>}>
                    <MonacoDiffEditor
                      key={`${selectedFile}:${prettify}`}
                      height={`100%`}
                      original={displayOrig}
                      modified={displayContent}
                      language={langFromPath(selectedFile)}
                      theme={editorTheme}
                      onMount={handleMonacoMount}
                      options={{
                        readOnly: true,
                        renderSideBySide: true,
                        minimap: {enabled: false},
                        fontSize: 13,
                        fontFamily: `'JetBrains Mono', monospace`,
                        scrollbar: {verticalScrollbarSize: 8, horizontalScrollbarSize: 8},
                        scrollBeyondLastLine: false,
                        padding: {top: 12},
                        contextmenu: false,
                      }}
                    />
                  </Suspense>
                ) : displayContent != null ? (
                  <Suspense fallback={<div className={`files-editor-empty`}><span>Loading editor…</span></div>}>
                    <MonacoEditor
                      key={`${selectedFile}:${prettify}`}
                      height={`100%`}
                      language={langFromPath(selectedFile)}
                      value={displayContent}
                      theme={editorTheme}
                      onMount={handleMonacoMount}
                      options={{
                        readOnly: true,
                        minimap: {enabled: false},
                        scrollBeyondLastLine: false,
                        fontSize: 13,
                        fontFamily: `'JetBrains Mono', monospace`,
                        lineNumbers: `on`,
                        renderLineHighlight: `line`,
                        scrollbar: {verticalScrollbarSize: 8, horizontalScrollbarSize: 8},
                        padding: {top: 12},
                        wordWrap: `on`,
                        contextmenu: false,
                      }}
                    />
                  </Suspense>
                ) : (
                  <div className={`files-editor-empty`}>
                    <span>Could not load file</span>
                  </div>
                )}
              </div>
            </>
          ) : (
            <div className={`files-editor-empty`}>
              {inCompare ? (
                <>
                  <CompareIcon/>
                  <span>Comparing <span className={`mono`}>{version}</span> ← <span className={`mono`}>{compareVersion}</span></span>
                  <span className={`text-[11px] text-[var(--fg-mute)]`}>{changeMap ? `${changedCount} file${changedCount !== 1 ? `s` : ``} changed` : `Loading…`}</span>
                </>
              ) : (
                <>
                  <OctIcon icon={oct[`file-directory`]} size={16}/>
                  <span>Select a file to view its contents</span>
                  <span className={`mono text-[11px] text-[var(--fg-mute)]`}>{name}@{version}</span>
                </>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
