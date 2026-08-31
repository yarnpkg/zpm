import {useMemo, type JSX}                                                                      from 'react';

import {SERIES_COLORS, median, getSeriesValues, type SeriesMeta, type Project, type BenchPoint} from './BenchmarksDashboard';

interface Props {
  data: Record<string, Record<string, Record<string, Array<BenchPoint>>>>;
  seriesOrder: ReadonlyArray<string>;
  seriesMeta: Record<string, SeriesMeta>;
  projects: Array<Project>;
  mutedSeries: Record<string, boolean>;
}

function aggregate(
  scenarioId: string,
  data: Props[`data`],
  seriesOrder: ReadonlyArray<string>,
  seriesMeta: Record<string, SeriesMeta>,
  projects: Array<Project>,
  mutedSeries: Record<string, boolean>,
) {
  const visibleSeries = seriesOrder.filter(sid => !mutedSeries[sid]);

  // Keep only projects where every visible series has a usable median.
  // Comparing geomeans across series is only meaningful when they're computed
  // over the same project set.
  const rows: Array<Record<string, number>> = [];
  for (const p of projects) {
    const projectData = data[scenarioId]?.[p.id];
    if (!projectData) continue;
    const medians: Record<string, number> = {};
    let complete = true;
    for (const sid of visibleSeries) {
      const m = median(getSeriesValues(projectData, sid));
      if (m > 0) {
        medians[sid] = m;
      } else {
        complete = false;
        break;
      }
    }
    if (complete) {
      rows.push(medians);
    }
  }

  if (visibleSeries.length === 0 || rows.length === 0)
    return [];


  // Geomean of absolute medians per series. Computed in log space for
  // numerical stability when projects span very different magnitudes.
  const geomean: Record<string, number> = {};
  for (const sid of visibleSeries) {
    let logSum = 0;
    for (const r of rows) logSum += Math.log(r[sid]);
    geomean[sid] = Math.exp(logSum / rows.length);
  }

  // Single normalization pass against the slowest aggregate, so the slowest
  // series lands on exactly 1.00× and the rest read as "X× the slowest."
  const slowest = Math.max(...visibleSeries.map(sid => geomean[sid]));

  const out = visibleSeries.map(sid => ({
    id: sid,
    name: seriesMeta[sid].name,
    normalized: geomean[sid] / slowest,
    color: SERIES_COLORS[sid],
    accent: seriesMeta[sid].accent,
  }));

  out.sort((a, b) => a.normalized - b.normalized);
  return out;
}

function SummaryCard({title, agg}: {title: string, agg: ReturnType<typeof aggregate>}): JSX.Element {
  return (
    <div className={`summary-card`}>
      <h4>{title}</h4>
      <div>
        {agg.map(a => (
          <div key={a.id} className={`summary-bar`}>
            <div className={`lbl${a.accent ? ` self` : ``}`}>{a.name}</div>
            <div className={`track`}>
              <div
                className={`fill${a.accent ? ` self` : ``}`}
                style={{
                  [`--w` as any]: a.normalized.toFixed(3),
                  ...(!a.accent ? {background: a.color} : {}),
                }}
              />
            </div>
            <div className={`val${a.accent ? ` self` : ``}`}>{a.normalized.toFixed(2)}&times;</div>
          </div>
        ))}
      </div>
    </div>
  );
}

export function BenchmarkSummary({data, seriesOrder, seriesMeta, projects, mutedSeries}: Props): JSX.Element {
  const coldAgg = useMemo(
    () => aggregate(`install-full-cold`, data, seriesOrder, seriesMeta, projects, mutedSeries),
    [data, seriesOrder, seriesMeta, projects, mutedSeries],
  );
  const warmAgg = useMemo(
    () => aggregate(`install-cache-and-lock`, data, seriesOrder, seriesMeta, projects, mutedSeries),
    [data, seriesOrder, seriesMeta, projects, mutedSeries],
  );

  return (
    <>
      <div className={`mt-12 mb-6`}>
        <div className={`mono text-[11px] text-[var(--fg-mute)] tracking-[0.12em] uppercase mb-2`}>&sect; Aggregate</div>
        <h2 className={`text-[26px] font-medium tracking-[-0.015em] m-0`}>Median across all scenarios.</h2>
        <p className={`text-[14.5px] text-[var(--fg-dim)] leading-[1.6] mt-2 max-w-[680px] text-pretty`}>
          Geometric mean of per-project medians for each series, then normalized so the slowest series reads as 1.00&times;. Every other series is the ratio of its average run time to the slowest. Lower is faster.
        </p>
      </div>
      <div className={`summary`}>
        <SummaryCard title={`Cold install · normalized`} agg={coldAgg} />
        <SummaryCard title={`Cache + lockfile · normalized`} agg={warmAgg} />
      </div>
    </>
  );
}
