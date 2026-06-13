import {useState, useEffect, useCallback, type JSX} from 'react';

import {BenchmarkChart}                             from './BenchmarkChart';
import {BenchmarkSummary}                           from './BenchmarkSummary';
import {BenchmarkTooltip, type HoverInfo}           from './BenchmarkTooltip';
import {useVersions}                                from './useVersions';

export interface BenchPoint {
  timestamp: number;
  value: number | null;
}

export interface SeriesMeta {
  name: string;
  dashed: boolean;
  accent: boolean;
}

export interface Project {
  id: string;
  name: string;
}

export interface Scenario {
  id: string;
  num: string;
  title: string;
  desc: string;
}

export interface Incident {
  start: number;
  end: number;
  label: string;
}

export const SERIES_COLORS: Record<string, string> = {
  zpm: `oklch(0.78 0.16 var(--accent-h))`,
  yarn: `oklch(0.65 0.10 var(--accent-h))`,
  npm: `oklch(0.70 0.15 25)`,
  pnpm: `oklch(0.75 0.13 220)`,
  classic: `oklch(0.55 0.08 var(--accent-h))`,
};

export function median(arr: Array<number | null>): number {
  const nums = arr.filter((v): v is number => v !== null && v !== undefined);
  if (!nums.length) return 0;
  nums.sort((a, b) => a - b);
  return nums[Math.floor(nums.length / 2)];
}

export function getSeriesValues(projectData: Record<string, Array<BenchPoint>>, pm: string): Array<number | null> {
  if (!projectData?.[pm]) return [];
  return projectData[pm].map(p => p.value);
}

interface Props {
  data: Record<string, Record<string, Record<string, Array<BenchPoint>>>>;
  seriesOrder: ReadonlyArray<string>;
  seriesMeta: Record<string, SeriesMeta>;
  projects: Array<Project>;
  scenarios: Array<Scenario>;
  incidents: Array<Incident>;
  benchMinTs: number;
  benchMaxTs: number;
}

const SWATCH_STYLES: Record<string, {dashed: boolean}> = {
  zpm: {dashed: false},
  yarn: {dashed: false},
  npm: {dashed: false},
  pnpm: {dashed: false},
  classic: {dashed: true},
};

export function BenchmarksDashboard({data, seriesOrder, seriesMeta, projects, scenarios, incidents, benchMinTs, benchMaxTs}: Props): JSX.Element {
  const [mutedSeries, setMutedSeries] = useState<Record<string, boolean>>({});
  const [selectedProject, setSelectedProject] = useState(`all`);
  const [showVersions, setShowVersions] = useState(false);
  const [hoverInfo, setHoverInfo] = useState<HoverInfo | null>(null);
  const [controlsOpen, setControlsOpen] = useState(false);

  const {versions, loading: versionsLoading} = useVersions(benchMinTs, benchMaxTs, showVersions);

  const visibleProjects = selectedProject === `all`
    ? projects
    : projects.filter(p => p.id === selectedProject);

  const toggleMute = useCallback((pm: string) => {
    setMutedSeries(prev => {
      if (prev[pm]) {
        const next = {...prev};
        delete next[pm];
        return next;
      }
      if (Object.keys(prev).length >= seriesOrder.length - 1) return prev;
      return {...prev, [pm]: true};
    });
  }, [seriesOrder.length]);

  const handleHover = useCallback((infoOrUpdater: HoverInfo | null | ((prev: HoverInfo | null) => HoverInfo | null)) => {
    if (typeof infoOrUpdater === `function`) {
      setHoverInfo(infoOrUpdater);
    } else {
      setHoverInfo(infoOrUpdater);
    }
  }, []);

  useEffect(() => {
    const dismiss = () => setHoverInfo(null);
    window.addEventListener(`scroll`, dismiss, {passive: true});
    return () => window.removeEventListener(`scroll`, dismiss);
  }, []);

  return (
    <>
      {/* Sticky controls */}
      <div className={`bench-sticky`}>
        <div className={`bench-controls${controlsOpen ? ` open` : ``}`}>
          <button className={`controls-toggle`} onClick={() => setControlsOpen(o => !o)}>
            <span className={`controls-summary`}>
              Filters
              <span className={`controls-badge`}>{projects.find(p => p.id === selectedProject)?.name ?? `All`}</span>
            </span>
            <span className={`controls-chevron`} />
          </button>
          <div className={`controls-body`}>
            {/* Project filter */}
            <div className={`filter-bar`}>
              <span className={`label`}>Project</span>
              {[{id: `all`, name: `All`}, ...projects].map(p => (
                <button
                  key={p.id}
                  className={`filter-pill${selectedProject === p.id ? ` active` : ``}`}
                  onClick={() => setSelectedProject(p.id)}
                >
                  {p.name}
                </button>
              ))}
            </div>

            {/* Series legend */}
            <div className={`legend-section`}>
              <span className={`label desktop-only`}>Series</span>
              {seriesOrder.map(sid => (
                <span key={sid} style={{display: `contents`}}>
                  {(sid === `npm` || sid === `classic`) && <span className={`sep`} />}
                  <button
                    className={`lg${mutedSeries[sid] ? ` muted` : ``}`}
                    style={{[`--c` as any]: SERIES_COLORS[sid]}}
                    onClick={() => toggleMute(sid)}
                  >
                    <span className={`swatch${SWATCH_STYLES[sid]?.dashed ? ` dashed` : ``}`} />
                    <span className={`name`}>{seriesMeta[sid].name}</span>
                  </button>
                </span>
              ))}
              <span className={`sep`} />
              <span className={`legend-hint`}>Click to mute · y-axis = seconds</span>
              <span className={`sep`} />
              <label className={`toggle-label${versionsLoading ? ` loading` : ``}`}>
                <input
                  type={`checkbox`}
                  checked={showVersions}
                  disabled={versionsLoading}
                  onChange={e => setShowVersions(e.target.checked)}
                />
                <span>Show versions</span>
                <i className={`version-spinner`} />
              </label>
            </div>
          </div>
        </div>
      </div>

      {/* Benchmark grid */}
      <div className={`bench-grid`} style={{[`--bench-cols` as any]: visibleProjects.length}}>
        <div className={`corner`}>scenario \ project</div>
        {visibleProjects.map(p => (
          <div key={p.id} className={`col-head`}>
            <span className={`name`}>{p.name}</span>
          </div>
        ))}

        {scenarios.map(sc => (
          <span key={sc.id} style={{display: `contents`}}>
            <div className={`row-head`}>
              <div className={`row-num`}>&sect; {sc.num} &middot; scenario</div>
              <h3>{sc.title}</h3>
              <p>{sc.desc}</p>
            </div>
            {visibleProjects.map(p => (
              <BenchmarkChart
                key={`${sc.id}-${p.id}`}
                scenario={sc}
                project={p}
                data={data[sc.id]?.[p.id] ?? {}}
                seriesOrder={seriesOrder}
                seriesMeta={seriesMeta}
                mutedSeries={mutedSeries}
                incidents={incidents}
                versions={versions}
                showVersions={showVersions && !versionsLoading}
                hoveredIndex={hoverInfo?.index ?? null}
                onHover={handleHover}
              />
            ))}
          </span>
        ))}
      </div>

      {/* Aggregate summary */}
      <BenchmarkSummary
        data={data}
        seriesOrder={seriesOrder}
        seriesMeta={seriesMeta}
        projects={projects}
        mutedSeries={mutedSeries}
      />

      {/* Tooltip */}
      <BenchmarkTooltip info={hoverInfo} />
    </>
  );
}
