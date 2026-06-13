import {useState, useEffect, useCallback, useMemo, useContext} from 'react';

import {AuditPanel}                                            from './AuditPanel';
import {DependenciesCard}                                      from './DependenciesCard';
import {DownloadsCard}                                         from './DownloadsCard';
import {FilesExplorer}                                         from './FilesExplorer';
import {InstallCard}                                           from './InstallCard';
import {KeywordsCard}                                          from './KeywordsCard';
import {LeftRail}                                              from './LeftRail';
import {MaintainersCard}                                       from './MaintainersCard';
import {ReadmePanel}                                           from './ReadmePanel';
import {StatGrid}                                              from './StatGrid';
import {TabBar}                                                from './TabBar';
import {VersionSelector}                                       from './VersionSelector';
import {VersionsCard}                                          from './VersionsCard';
import {VersionsTimeline}                                      from './VersionsTimeline';
import {PackageCtx, IconCtx}                                   from './contexts';
import {OctIcon}                                               from './icons';
import {usePackageNavigate, splatRoute}                        from './router';
import type {RegistryData, FileEntry, DownloadDay, Tab, PmTab} from './types';
import {parseSplat, packagePath, getLicense, getRepoUrl}       from './utils';

function LoadingSpinner() {
  return (
    <div className={`max-w-[1400px] mx-auto px-7 pt-32 pb-20 flex flex-col items-center gap-4`}>
      <div className={`w-8 h-8 border-2 border-[var(--line-strong)] border-t-[var(--accent)] rounded-full`} style={{animation: `pkg-spin 0.6s linear infinite`}}/>
      <div className={`text-sm text-[var(--fg-mute)]`}>Loading package...</div>
    </div>
  );
}

function ErrorDisplay({message}: {message: string}) {
  return (
    <div className={`max-w-[1400px] mx-auto px-7 pt-32 pb-20 flex flex-col items-center gap-4`}>
      <div className={`w-12 h-12 border border-[var(--line-strong)] rounded-full inline-flex items-center justify-center text-[var(--fg-mute)]`}>!</div>
      <div className={`text-lg text-[var(--fg)]`}>Package not found</div>
      <div className={`text-sm text-[var(--fg-mute)]`}>{message}</div>
    </div>
  );
}

export function PackagePageInner() {
  const {brandIcons, octicons} = useContext(PackageCtx);
  const nav = usePackageNavigate();
  const {_splat: splat} = splatRoute.useParams();
  const parsed = useMemo(() => parseSplat(splat ?? ``), [splat]);

  const name = parsed.name;
  const urlVersion = parsed.version;
  const activeTab: Tab = parsed.tab ?? (parsed.compareVersion ? `files` : `readme`);
  const urlFile = parsed.filePath ?? null;
  const urlCompare = parsed.compareVersion ?? null;
  const activeNav = activeTab === `versions` ? `versions` : activeTab === `files` ? `files` : `info`;

  const [registry, setRegistry] = useState<RegistryData | null>(null);
  const [files, setFiles] = useState<Array<FileEntry> | null>(null);
  const [readme, setReadme] = useState(``);
  const [downloads, setDownloads] = useState<Array<DownloadDay> | null>(null);
  const [pmTab, setPmTab] = useState<PmTab>(`yarn`);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const selectedVersion = urlVersion
    || (registry ? registry[`dist-tags`]?.latest || Object.keys(registry.versions).pop() || `` : ``);

  useEffect(() => {
    if (!name) {
      setLoading(false);
      return () => {};
    }

    const abortCtrl = new AbortController();
    setLoading(true);
    setError(null);

    fetch(`https://registry.yarnpkg.com/${name}`, {signal: abortCtrl.signal})
      .then(r => {
        if (!r.ok) throw new Error(r.status === 404 ? `Package "${name}" not found` : `Failed to fetch package`);
        return r.json();
      })
      .then((data: RegistryData) => {
        setRegistry(data);
        if (data.readme)
          setReadme(data.readme);

        document.title = `${data.name} — Yarn`;
      })
      .catch(err => {
        if (err.name !== `AbortError`) {
          setError(err.message);
        }
      })
      .finally(() => setLoading(false));
    return () => {
      abortCtrl.abort();
    };
  }, [name]);

  useEffect(() => {
    if (!selectedVersion || !name)
      return undefined;


    const abortCtrl = new AbortController();
    setFiles(null);

    fetch(`https://data.jsdelivr.com/v1/package/npm/${name}@${selectedVersion}/flat`, {signal: abortCtrl.signal})
      .then(r => r.ok ? r.json() : null)
      .then(data => {
        if (data?.files) {
          setFiles(data.files);
        }
      })
      .catch(err => {
        if (err.name !== `AbortError`) {
          setFiles(null);
        }
      });

    fetch(`https://cdn.jsdelivr.net/npm/${name}@${selectedVersion}/README.md`, {signal: abortCtrl.signal})
      .then(r => {
        if (!r.ok) throw new Error(`no readme`);
        return r.text();
      })
      .then(text => setReadme(text))
      .catch(err => {
        if (err.name === `AbortError`)
          return;

        fetch(`https://cdn.jsdelivr.net/npm/${name}@${selectedVersion}/readme.md`, {signal: abortCtrl.signal})
          .then(r => r.ok ? r.text() : ``)
          .then(text => {
            if (text) {
              setReadme(text);
            }
          })
          .catch(() => {});
      });

    return () => abortCtrl.abort();
  }, [name, selectedVersion]);

  useEffect(() => {
    if (!name)
      return undefined;

    const abortCtrl = new AbortController();
    fetch(`https://api.npmjs.org/downloads/range/last-month/${name}`, {signal: abortCtrl.signal})
      .then(r => r.ok ? r.json() : null)
      .then(data => {
        if (data?.downloads) {
          setDownloads(data.downloads);
        }
      })
      .catch(err => {
        if (err.name !== `AbortError`) {
          setDownloads(null);
        }
      });
    return () => abortCtrl.abort();
  }, [name]);

  const handleVersionChange = useCallback((v: string) => {
    const filePath = activeTab === `files` ? urlFile : undefined;
    nav(packagePath(name, v, activeTab, filePath ?? undefined));
  }, [name, activeTab, nav, urlFile]);

  const handleNavClick = useCallback((id: string) => {
    const tab: Tab = id === `versions` ? `versions` : id === `files` ? `files` : `readme`;
    const cmp = tab === `files` ? urlCompare ?? undefined : undefined;
    nav(packagePath(name, selectedVersion, tab, undefined, cmp));
  }, [name, selectedVersion, nav, urlCompare]);

  const handleTabChange = useCallback((t: Tab) => {
    const cmp = t === `files` ? urlCompare ?? undefined : undefined;
    nav(packagePath(name, selectedVersion, t, undefined, cmp));
  }, [name, selectedVersion, nav, urlCompare]);

  const handleFileChange = useCallback((filePath: string | null) => {
    nav(packagePath(name, selectedVersion, `files`, filePath ?? undefined, urlCompare ?? undefined));
  }, [name, selectedVersion, nav, urlCompare]);

  const handleCompareChange = useCallback((cmpVersion: string | null) => {
    nav(packagePath(name, selectedVersion, `files`, urlFile ?? undefined, cmpVersion ?? undefined));
  }, [name, selectedVersion, nav, urlFile]);

  useEffect(() => {
    document.body.style.overflow = activeTab === `files` ? `hidden` : ``;
    return () => {
      document.body.style.overflow = ``;
    };
  }, [activeTab]);

  const iconCtxValue = useMemo(() => ({brand: brandIcons, oct: octicons}), [brandIcons, octicons]);

  if (!name)
    return <ErrorDisplay message={`No package name specified in the URL.`}/>;

  if (loading) return <LoadingSpinner/>;
  if (error || !registry) return <ErrorDisplay message={error || `Package not found`}/>;

  const allVersions = Object.keys(registry.versions);
  const distTags = registry[`dist-tags`] || {};
  const versionData = registry.versions[selectedVersion];
  const repoUrl = getRepoUrl(registry.repository);
  const license = getLicense(registry.license);
  const deps = versionData?.dependencies || {};
  const peerDeps = versionData?.peerDependencies || {};
  const keywords = registry.keywords || [];
  const maintainers = registry.maintainers || [];

  if (activeTab === `files`) {
    return (
      <IconCtx.Provider value={iconCtxValue}>
        <div className={`overflow-hidden`} style={{animation: `pkg-fade-in 0.3s ease-out`}}>
          <FilesExplorer
            files={files}
            name={name}
            version={selectedVersion}
            versions={allVersions}
            distTags={distTags}
            time={registry.time}
            onVersionChange={handleVersionChange}
            onTabChange={handleTabChange}
            onFileChange={handleFileChange}
            onCompareChange={handleCompareChange}
            selectedFile={urlFile}
            compareVersion={urlCompare}
          />
        </div>
      </IconCtx.Provider>
    );
  }

  return (
    <IconCtx.Provider value={iconCtxValue}>
      <div className={`max-w-[1400px] mx-auto px-7 pt-9 pb-20`} style={{animation: `pkg-fade-in 0.3s ease-out`}}>
        {/* Breadcrumb */}
        <div className={`flex items-center gap-2 mono text-[11px] text-[var(--fg-mute)] tracking-[0.1em] uppercase mb-5`}>
          <a href={`/package`} className={`no-underline text-inherit hover:text-[var(--fg-dim)]`}>packages</a>
          <span className={`opacity-40`}>/</span>
          <span className={`text-[var(--fg-dim)]`}>{name}</span>
        </div>

        <div className={`grid grid-cols-1 lg:grid-cols-[240px_1fr_320px] gap-9 items-start`}>
          {/* Left Rail */}
          <LeftRail
            name={name}
            version={selectedVersion}
            distTags={distTags}
            homepage={registry.homepage || undefined}
            repoUrl={repoUrl}
            activeNav={activeNav}
            onNavClick={handleNavClick}
            versionCount={allVersions.length}
            fileCount={files?.length ?? 0}
          />

          {/* Main Column */}
          <main className={`min-w-0`}>
            {/* Header */}
            <header className={`flex items-start gap-6 mb-2 flex-wrap`}>
              <div className={`flex-1 min-w-0`}>
                <h1 className={`mono text-[clamp(34px,4.6vw,54px)] font-medium tracking-[-0.025em] text-[var(--fg)] leading-[0.95] m-0 mb-2.5`}>
                  {name}
                </h1>

                {registry.description && (
                  <p className={`text-[17px] text-[var(--fg-dim)] leading-relaxed max-w-[640px] m-0 mb-5`}>
                    {registry.description}
                  </p>
                )}

                <div className={`flex items-center gap-3.5 flex-wrap text-[12.5px] text-[var(--fg-mute)]`}>
                  <span className={`inline-flex items-center gap-1.5 py-1 px-2.5 rounded-full bg-[var(--accent-soft)] border border-[var(--accent-line)] text-[var(--accent)] no-underline`}>
                    <OctIcon icon={octicons.law} size={11}/>
                    {license}
                  </span>

                  {repoUrl && (
                    <a href={repoUrl} target={`_blank`} rel={`noopener noreferrer`}
                      className={`inline-flex items-center gap-1.5 py-1 px-2.5 rounded-full border border-[var(--line-strong)] text-[var(--fg-dim)] no-underline transition-colors hover:text-[var(--fg)] hover:border-[var(--fg-mute)]`}
                    >
                      <OctIcon icon={octicons.repo} size={11}/>
                      {repoUrl.replace(/^https?:\/\/(www\.)?github\.com\//, ``)}
                    </a>
                  )}

                  {registry.homepage && (
                    <a href={registry.homepage} target={`_blank`} rel={`noopener noreferrer`}
                      className={`inline-flex items-center gap-1.5 py-1 px-2.5 rounded-full border border-[var(--line-strong)] text-[var(--fg-dim)] no-underline transition-colors hover:text-[var(--fg)] hover:border-[var(--fg-mute)]`}
                    >
                      <OctIcon icon={octicons[`link-external`]} size={11}/>
                      homepage
                    </a>
                  )}
                </div>
              </div>

              <VersionSelector
                version={selectedVersion}
                distTags={distTags}
                versions={allVersions}
                time={registry.time}
                onVersionChange={handleVersionChange}
              />
            </header>

            <TabBar
              active={activeTab}
              onTabChange={handleTabChange}
              readmeLabel={readme ? `M` : `—`}
              versionCount={allVersions.length}
              fileCount={files?.length ?? 0}
            />

            <StatGrid
              versionData={versionData}
              time={registry.time}
              version={selectedVersion}
              files={files}
            />

            {activeTab === `readme` && (
              <>
                <InstallCard name={name} pmTab={pmTab} onPmTabChange={setPmTab}/>

                <ReadmePanel readme={readme} name={name}/>
              </>
            )}

            {activeTab === `versions` && (
              <VersionsTimeline versions={allVersions} distTags={distTags} time={registry.time}/>
            )}

            {activeTab === `audit` && (
              <AuditPanel/>
            )}
          </main>

          {/* Right Rail */}
          <aside className={`flex flex-col gap-[18px] lg:sticky lg:top-[90px]`}>
            <DownloadsCard downloads={downloads}/>

            <VersionsCard
              versions={allVersions}
              distTags={distTags}
              time={registry.time}
              onVersionChange={handleVersionChange}
            />

            <DependenciesCard deps={deps} title={`Dependency`}/>

            {Object.keys(peerDeps).length > 0 && (
              <DependenciesCard deps={peerDeps} title={`Peer dependency`}/>
            )}

            <MaintainersCard maintainers={maintainers}/>

            <KeywordsCard keywords={keywords}/>
          </aside>
        </div>
      </div>
    </IconCtx.Provider>
  );
}
