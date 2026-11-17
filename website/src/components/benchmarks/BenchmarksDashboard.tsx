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

export interface Platform {
  id: string;
  name: string;
}

// Defined per theme in benchmarks.astro: light blue for Yarn 6, dark blue for
// Yarn 4 and Yarn Classic (which is dashed), red for npm, orange for pnpm
export const SERIES_COLORS: Record<string, string> = {
  zpm: `var(--series-zpm)`,
  yarn: `var(--series-yarn)`,
  npm: `var(--series-npm)`,
  pnpm: `var(--series-pnpm)`,
  classic: `var(--series-classic)`,
};

// Series that are hidden until their legend entry is clicked
const DEFAULT_MUTED_SERIES: Record<string, boolean> = {
  classic: true,
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
  // platform -> scenario -> project -> series -> points
  data: Record<string, Record<string, Record<string, Record<string, Array<BenchPoint>>>>>;
  seriesOrder: ReadonlyArray<string>;
  seriesMeta: Record<string, SeriesMeta>;
  projects: Array<Project>;
  platforms: Array<Platform>;
  defaultPlatform: string;
  scenarios: Array<Scenario>;
  incidents: Array<Incident>;
  benchMinTs: number;
  benchMaxTs: number;
}

const EMPTY_PLATFORM_DATA: Props[`data`][string] = {};
const EMPTY_PROJECT_DATA: Record<string, Array<BenchPoint>> = {};

export function BenchmarksDashboard({data, seriesOrder, seriesMeta, projects, platforms, defaultPlatform, scenarios, incidents, benchMinTs, benchMaxTs}: Props): JSX.Element {
  const [mutedSeries, setMutedSeries] = useState<Record<string, boolean>>(DEFAULT_MUTED_SERIES);
  const [selectedProject, setSelectedProject] = useState(`all`);
  const [selectedPlatform, setSelectedPlatform] = useState(defaultPlatform);
  const [showVersions, setShowVersions] = useState(false);
  const [excludeOutliers, setExcludeOutliers] = useState(false);
  const [hoverInfo, setHoverInfo] = useState<HoverInfo | null>(null);
  const [controlsOpen, setControlsOpen] = useState(false);

  const {versions, loading: versionsLoading} = useVersions(benchMinTs, benchMaxTs, showVersions);

  const visibleProjects = selectedProject === `all`
    ? projects
    : projects.filter(p => p.id === selectedProject);

  const platformData = data[selectedPlatform] ?? EMPTY_PLATFORM_DATA;

  const selectPlatform = useCallback((platform: string) => {
    // The hovered index refers to the time axis of the previous platform
    setHoverInfo(null);
    setSelectedPlatform(platform);
  }, []);

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
              <span className={`controls-badge`}>{platforms.find(p => p.id === selectedPlatform)?.name}</span>
            </span>
            <span className={`controls-chevron`} />
          </button>
          <div className={`controls-body`}>
            <div className={`filter-row`}>
              {/* Project filter */}
              <div className={`filter-bar`}>
                <span className={`label`}>Project</span>
                {[{id: `all`, name: `All`}, ...projects].map(p => (
                  <button
                    key={p.id}
                    className={`filter-pill${selectedProject === p.id ? ` active` : ``}`}
                    aria-pressed={selectedProject === p.id}
                    onClick={() => setSelectedProject(p.id)}
                  >
                    {p.name}
                  </button>
                ))}
              </div>

              {/* Platform toggle */}
              <div className={`filter-bar`}>
                <span className={`label`}>Platform</span>
                {platforms.map(p => (
                  <button
                    key={p.id}
                    className={`filter-pill${selectedPlatform === p.id ? ` active` : ``}`}
                    aria-pressed={selectedPlatform === p.id}
                    onClick={() => selectPlatform(p.id)}
                  >
                    {p.name}
                  </button>
                ))}
              </div>
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
                    <span className={`swatch${seriesMeta[sid].dashed ? ` dashed` : ``}`} />
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
              <label className={`toggle-label`}>
                <input
                  type={`checkbox`}
                  checked={excludeOutliers}
                  onChange={e => setExcludeOutliers(e.target.checked)}
                />
                <span>Exclude outliers</span>
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
                data={platformData[sc.id]?.[p.id] ?? EMPTY_PROJECT_DATA}
                excludeOutliers={excludeOutliers}
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
        data={platformData}
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
