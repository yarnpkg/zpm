import {useState, useEffect, useMemo, useRef, useCallback, type JSX}                                                          from 'react';

import type {HoverInfo}                                                                                                       from './BenchmarkTooltip';
import {SERIES_COLORS, median, getSeriesValues, type SeriesMeta, type Scenario, type Project, type Incident, type BenchPoint} from './BenchmarksDashboard';
import type {VersionEntry}                                                                                                    from './useVersions';

const ML = 30, MR = 6, MT = 8, MB = 18;

const GITHUB_REPOS: Record<string, {repo: string, tagPrefix: string}> = {
  npm: {repo: `npm/cli`, tagPrefix: `v`},
  pnpm: {repo: `pnpm/pnpm`, tagPrefix: `v`},
  classic: {repo: `yarnpkg/yarn`, tagPrefix: `v`},
  yarn: {repo: `yarnpkg/berry`, tagPrefix: `@yarnpkg/cli/`},
};

interface Props {
  scenario: Scenario;
  project: Project;
  data: Record<string, Array<BenchPoint>>;
  seriesOrder: ReadonlyArray<string>;
  seriesMeta: Record<string, SeriesMeta>;
  mutedSeries: Record<string, boolean>;
  incidents: Array<Incident>;
  versions: Record<string, Array<VersionEntry>> | null;
  showVersions: boolean;
  hoveredIndex: number | null;
  onHover: (info: HoverInfo | null | ((prev: HoverInfo | null) => HoverInfo | null)) => void;
}

export function BenchmarkChart({scenario, project, data, seriesOrder, seriesMeta, mutedSeries, incidents, versions, showVersions, hoveredIndex, onHover}: Props): JSX.Element {
  const svgRef = useRef<SVGSVGElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState<{w: number, h: number} | null>(null);

  useEffect(() => {
    const el = containerRef.current;
    if (!el)
      return () => {};

    const ro = new ResizeObserver(entries => {
      const {width, height} = entries[0].contentRect;
      setSize({w: width, h: height});
    });
    ro.observe(el);
    return () => {
      ro.disconnect();
    };
  }, []);

  const chartData = useMemo(() => {
    if (!size)
      return null;


    const {w, h} = size;
    const pw = w - ML - MR;
    const ph = h - MT - MB;

    if (pw <= 0 || ph <= 0)
      return null;

    const visible = seriesOrder.filter(s => !mutedSeries[s]);
    const allVals: Array<number> = [];
    for (const sid of visible) {
      const vals = getSeriesValues(data, sid);
      for (const v of vals) {
        if (v !== null) {
          allVals.push(v);
        }
      }
    }

    if (!allVals.length)
      return null;


    const yMin = 0;
    let yMax = Math.max(...allVals);
    const pad = yMax * 0.12 || 0.1;
    yMax = yMax + pad;

    const points = data[seriesOrder[0]];
    const N = points?.length ?? 0;
    if (!N)
      return null;


    const xScale = (i: number) => ML + (i / (N - 1)) * pw;
    const yScale = (v: number) => MT + ph - ((v - yMin) / (yMax - yMin)) * ph;

    const incidentSet: Record<number, boolean> = {};
    const incidentRanges: Array<{start: number, end: number, label: string}> = [];

    for (const inc of incidents) {
      let iStart = -1, iEnd = -1;
      for (let ip = 0; ip < N; ip++) {
        const ts = points[ip].timestamp;
        if (ts >= inc.start && iStart === -1)
          iStart = ip;

        if (ts <= inc.end) {
          iEnd = ip;
        }
      }
      if (iStart === -1 || iEnd === -1 || iEnd < iStart)
        continue;

      incidentRanges.push({start: iStart, end: iEnd, label: inc.label});
      for (let ik = iStart; ik <= iEnd; ik++) {
        incidentSet[ik] = true;
      }
    }

    const drawOrder = seriesOrder.filter(s => s !== `zpm`).concat(`zpm`);

    const paths: Array<{id: string, d: string, cls: string, color: string}> = [];
    for (const sid of drawOrder) {
      if (mutedSeries[sid]) continue;
      const seriesPoints = data[sid];
      if (!seriesPoints) continue;

      let pathD = ``;
      let prevWasNull = true;
      for (let pi = 0; pi < seriesPoints.length; pi++) {
        const sv = seriesPoints[pi].value;
        if (sv === null || incidentSet[pi]) {
          prevWasNull = true; continue;
        }
        const px = xScale(pi), py = yScale(sv);
        pathD += `${(prevWasNull ? `M` : `L`) + px.toFixed(2)},${py.toFixed(2)}`;
        prevWasNull = false;
      }
      if (!pathD) continue;

      const meta = seriesMeta[sid];
      const cls = `series-line${meta.dashed ? ` dashed` : ``}${meta.accent ? ` highlight` : ``}`;
      paths.push({id: sid, d: pathD, cls, color: SERIES_COLORS[sid]});
    }

    let band: {x: number, y: number, w: number, h: number} | null = null;
    if (!mutedSeries.zpm) {
      const zpmVals = getSeriesValues(data, `zpm`);
      const zpmMed = median(zpmVals);
      const top = yScale(zpmMed * 1.08);
      const bot = yScale(zpmMed * 0.92);
      if (bot > top) {
        band = {x: ML, y: top, w: pw, h: bot - top};
      }
    }

    const versionDots: Array<{cx: number, cy: number, r: number, color: string, cls: string, url: string | null}> = [];
    if (showVersions && versions) {
      for (const sid of drawOrder) {
        if (mutedSeries[sid])
          continue;

        const vers = versions[sid] ?? [];
        const seriesP = data[sid];
        if (!seriesP || !vers.length) continue;

        for (let vi = 0; vi < vers.length; vi++) {
          const ver = vers[vi];
          let bestIdx = 0, bestDist = Infinity;
          for (let vp = 0; vp < N; vp++) {
            const dist = Math.abs(points[vp].timestamp - ver.t);
            if (dist < bestDist) {
              bestDist = dist; bestIdx = vp;
            }
          }
          if (incidentSet[bestIdx]) continue;
          const sv = seriesP[bestIdx]?.value;
          if (sv === null || sv === undefined) continue;

          const gh = GITHUB_REPOS[sid];
          let url: string | null = null;
          if (gh) {
            if (vi > 0) {
              url = `https://github.com/${gh.repo}/compare/${gh.tagPrefix}${vers[vi - 1].v}...${gh.tagPrefix}${ver.v}`;
            } else {
              url = `https://github.com/${gh.repo}/releases/tag/${gh.tagPrefix}${ver.v}`;
            }
          }

          versionDots.push({
            cx: xScale(bestIdx),
            cy: yScale(sv),
            r: seriesMeta[sid].accent ? 4 : 3,
            color: SERIES_COLORS[sid],
            cls: `version-dot`,
            url,
          });
        }
      }
    }

    const yTicks = [yMin, (yMin + yMax) / 2, yMax].map(v => ({
      value: v,
      label: `${v < 1 ? v.toFixed(2) : v < 10 ? v.toFixed(1) : Math.round(v).toString()}s`,
      pct: (yScale(v) / h * 100),
    }));

    const dateIndices = [0, Math.floor(N / 4), Math.floor(N / 2), Math.floor(3 * N / 4), N - 1];
    const xLabels = dateIndices.map(idx => {
      const ts = points[idx].timestamp;
      const d = new Date(ts * 1000);
      return {
        label: `${d.getMonth() + 1}/${d.getDate()}`,
        pct: (xScale(idx) / w * 100),
      };
    });

    return {
      w, h, pw, ph,
      yMin, yMax, N, points, xScale, yScale,
      incidentSet, incidentRanges,
      paths, band, versionDots,
      yTicks, xLabels,
      drawOrder,
    };
  }, [data, seriesOrder, seriesMeta, mutedSeries, incidents, versions, showVersions, size]);

  const zpmValues = useMemo(() => getSeriesValues(data, `zpm`), [data]);
  const zpmMedian = useMemo(() => median(zpmValues), [zpmValues]);

  const pill = useMemo(() => {
    if (zpmMedian <= 0)
      return null;

    const medians: Array<{id: string, m: number}> = [];
    for (const sid of seriesOrder) {
      if (mutedSeries[sid])
        continue;

      const m = median(getSeriesValues(data, sid));
      if (m > 0) {
        medians.push({id: sid, m});
      }
    }

    const others = medians.filter(x => x.id !== `zpm`);
    if (!others.length)
      return {cls: `fastest`, text: `no comparison data`};


    const fastest = others.reduce((min, x) => x.m < min.m ? x : min, others[0]);
    const name = seriesMeta[fastest.id]?.name ?? fastest.id;

    if (zpmMedian <= fastest.m) {
      const diff = +(fastest.m - zpmMedian).toFixed(1);
      if (diff === 0)
        return {cls: `contested`, text: `tied with ${name}`};

      return {cls: `fastest`, text: `${diff}s faster than ${name}`};
    }

    const diff = +(zpmMedian - fastest.m).toFixed(1);

    const cls = diff / zpmMedian <= 0.1 ? `contested` : `slower`;
    return {cls, text: `${diff}s slower than ${name}`};
  }, [data, seriesOrder, seriesMeta, mutedSeries, zpmMedian]);

  const prevIdxRef = useRef<number | null>(null);

  const handleMouseMove = useCallback((e: React.MouseEvent) => {
    if (!chartData || !svgRef.current)
      return;

    const rect = svgRef.current.getBoundingClientRect();
    const sx = e.clientX - rect.left;

    if (sx < ML || sx > chartData.w - MR) {
      prevIdxRef.current = null;
      onHover(null);
      return;
    }

    const tFrac = (sx - ML) / chartData.pw;
    const idx = Math.max(0, Math.min(chartData.N - 1, Math.round(tFrac * (chartData.N - 1))));

    if (idx === prevIdxRef.current) {
      onHover(prev => prev ? {...prev, mouseX: e.clientX, mouseY: e.clientY, index: idx} : prev);
      return;
    }
    prevIdxRef.current = idx;

    const ts = chartData.points[idx].timestamp;
    const d = new Date(ts * 1000);
    const dateStr = d.toISOString().slice(0, 10);
    const inIncident = !!chartData.incidentSet[idx];

    if (inIncident) {
      let incLabel = ``;
      for (const ir of chartData.incidentRanges) {
        if (idx >= ir.start && idx <= ir.end) {
          incLabel = ir.label;
          break;
        }
      }

      onHover({
        mouseX: e.clientX, mouseY: e.clientY, index: idx,
        dateStr, scenarioTitle: scenario.title, projectName: project.name,
        isIncident: true, incidentLabel: incLabel,
        rows: [], versionMap: null, showVersions, seriesMeta,
      });
      return;
    }

    const rows: Array<{id: string, value: number}> = [];
    for (const sid of seriesOrder) {
      if (mutedSeries[sid]) continue;
      const sp = data[sid];
      if (!sp?.[idx] || sp[idx].value === null) continue;
      rows.push({id: sid, value: sp[idx].value!});
    }
    rows.sort((a, b) => a.value - b.value);

    let versionMap: Record<string, string> | null = null;
    if (showVersions && versions) {
      versionMap = {};
      for (const sid of seriesOrder) {
        const vers = versions[sid];
        if (!vers?.length) continue;
        for (let i = vers.length - 1; i >= 0; i--) {
          if (vers[i].t <= ts) {
            versionMap[sid] = vers[i].v; break;
          }
        }
      }
    }

    onHover({
      mouseX: e.clientX, mouseY: e.clientY, index: idx,
      dateStr, scenarioTitle: scenario.title, projectName: project.name,
      isIncident: false,
      rows, versionMap, showVersions, seriesMeta,
    });
  }, [chartData, data, seriesOrder, seriesMeta, mutedSeries, scenario, project, versions, showVersions, onHover]);

  const handleMouseLeave = useCallback(() => {
    prevIdxRef.current = null;
    onHover(null);
  }, [onHover]);

  const cellRef = useRef<HTMLDivElement>(null);
  const touchDepsRef = useRef({chartData, data, seriesOrder, seriesMeta, mutedSeries, scenario, project, versions, showVersions, onHover});
  touchDepsRef.current = {chartData, data, seriesOrder, seriesMeta, mutedSeries, scenario, project, versions, showVersions, onHover};

  useEffect(() => {
    const el = cellRef.current;
    if (!el)
      return () => {};

    const onTouch = (e: TouchEvent) => {
      const {chartData: cd, data: d, seriesOrder: so, seriesMeta: sm, mutedSeries: ms, scenario: sc, project: pr, versions: ver, showVersions: sv, onHover: oh} = touchDepsRef.current;
      if (!cd || !svgRef.current) return;
      e.preventDefault();
      const touch = e.touches[0];
      if (!touch) {
        prevIdxRef.current = null; oh(null); return;
      }
      const rect = svgRef.current.getBoundingClientRect();
      const sx = touch.clientX - rect.left;

      if (sx < ML || sx > cd.w - MR) {
        prevIdxRef.current = null;
        oh(null);
        return;
      }

      const tFrac = (sx - ML) / cd.pw;
      const idx = Math.max(0, Math.min(cd.N - 1, Math.round(tFrac * (cd.N - 1))));

      if (idx === prevIdxRef.current) {
        oh((prev: any) => prev ? {...prev, mouseX: touch.clientX, mouseY: touch.clientY, index: idx} : prev);
        return;
      }
      prevIdxRef.current = idx;

      const ts = cd.points[idx].timestamp;
      const dt = new Date(ts * 1000);
      const dateStr = dt.toISOString().slice(0, 10);
      const inIncident = !!cd.incidentSet[idx];

      if (inIncident) {
        let incLabel = ``;
        for (const ir of cd.incidentRanges) {
          if (idx >= ir.start && idx <= ir.end) {
            incLabel = ir.label;
            break;
          }
        }

        oh({mouseX: touch.clientX, mouseY: touch.clientY, index: idx, dateStr, scenarioTitle: sc.title, projectName: pr.name, isIncident: true, incidentLabel: incLabel, rows: [], versionMap: null, showVersions: sv, seriesMeta: sm});
        return;
      }

      const rows: Array<{id: string, value: number}> = [];
      for (const sid of so) {
        if (ms[sid])
          continue;

        const sp = d[sid];
        if (!sp?.[idx] || sp[idx].value === null)
          continue;

        rows.push({id: sid, value: sp[idx].value!});
      }
      rows.sort((a, b) => a.value - b.value);

      let versionMap: Record<string, string> | null = null;
      if (sv && ver) {
        versionMap = {};
        for (const sid of so) {
          const vs = ver[sid];
          if (!vs?.length)
            continue;

          for (let i = vs.length - 1; i >= 0; i--) {
            if (vs[i].t <= ts) {
              versionMap[sid] = vs[i].v;
              break;
            }
          }
        }
      }

      oh({mouseX: touch.clientX, mouseY: touch.clientY, index: idx, dateStr, scenarioTitle: sc.title, projectName: pr.name, isIncident: false, rows, versionMap, showVersions: sv, seriesMeta: sm});
    };

    const onEnd = () => {
      prevIdxRef.current = null;
      touchDepsRef.current.onHover(null);
    };

    el.addEventListener(`touchstart`, onTouch, {passive: false});
    el.addEventListener(`touchmove`, onTouch, {passive: false});
    el.addEventListener(`touchend`, onEnd);
    return () => {
      el.removeEventListener(`touchstart`, onTouch);
      el.removeEventListener(`touchmove`, onTouch);
      el.removeEventListener(`touchend`, onEnd);
    };
  }, []);

  if (!chartData) {
    return (
      <div className={`chart-cell`}>
        <div className={`cell-project`}>{project.name}</div>
        <div className={`cell-meta`}>
          <span className={`median`}>{size ? `No data` : ``}</span>
        </div>
        <div ref={containerRef} style={{position: `relative`, flex: 1, overflow: `visible`}} />
      </div>
    );
  }

  const gridY = [0.25, 0.5, 0.75];

  return (
    <div ref={cellRef} className={`chart-cell`} onMouseMove={handleMouseMove} onMouseLeave={handleMouseLeave}>
      <div className={`cell-project`}>{project.name}</div>
      <div className={`cell-meta`}>
        <span className={`median`}>yarn median <b>{zpmMedian.toFixed(2)}s</b></span>
        {pill && <span className={`cell-pill ${pill.cls}`}>{pill.text}</span>}
      </div>
      <div ref={containerRef} style={{position: `relative`, flex: 1, overflow: `visible`}}>
        <svg ref={svgRef} style={{width: `100%`, height: `100%`, display: `block`, overflow: `visible`}}>
          {gridY.map(f => {
            const gy = MT + f * chartData.ph;
            return <line key={f} x1={ML} x2={chartData.w - MR} y1={gy} y2={gy} className={`ax-line`} />;
          })}

          <line x1={ML} x2={ML} y1={MT} y2={MT + chartData.ph} className={`ax-line-strong`} />
          <line x1={ML} x2={chartData.w - MR} y1={MT + chartData.ph} y2={MT + chartData.ph} className={`ax-line-strong`} />

          {chartData.incidentRanges.map((ir, i) => {
            const ix1 = chartData.xScale(ir.start);
            const ix2 = chartData.xScale(ir.end);
            const iw = Math.max(ix2 - ix1, 2);
            return (
              <g key={i}>
                <rect x={ix1} y={MT} width={iw} height={chartData.ph} className={`incident-area`} />
                <rect x={ix1} y={MT} width={iw} height={chartData.ph} className={`incident-border`} />
              </g>
            );
          })}

          {chartData.band && (
            <rect x={chartData.band.x} y={chartData.band.y} width={chartData.band.w} height={chartData.band.h} className={`ax-band`} />
          )}

          {chartData.paths.map(p => (
            <path key={p.id} d={p.d} className={p.cls} stroke={p.color} style={{[`--c` as any]: p.color}} />
          ))}

          {chartData.versionDots.map((dot, i) =>
            dot.url ? (
              <a key={i} href={dot.url} target={`_blank`} rel={`noopener noreferrer`} className={`version-dot-link`}>
                <circle cx={dot.cx} cy={dot.cy} r={dot.r} fill={dot.color} className={dot.cls} />
              </a>
            ) : (
              <circle key={i} cx={dot.cx} cy={dot.cy} r={dot.r} fill={dot.color} className={dot.cls} />
            ),
          )}

          {hoveredIndex !== null && hoveredIndex < chartData.N && (
            <line
              x1={chartData.xScale(hoveredIndex)}
              x2={chartData.xScale(hoveredIndex)}
              y1={MT}
              y2={MT + chartData.ph}
              className={`crosshair`}
            />
          )}
        </svg>

        {chartData.yTicks.map((tick, i) => (
          <span key={i} className={`ax-label ax-label-y`} style={{top: `${tick.pct}%`, left: `${(ML - 4) / chartData.w * 100}%`}}>
            {tick.label}
          </span>
        ))}

        {chartData.xLabels.map((lbl, i) => (
          <span key={i} className={`ax-label ax-label-x`} style={{left: `${lbl.pct}%`, top: `${(MT + chartData.ph + 6) / chartData.h * 100}%`}}>
            {lbl.label}
          </span>
        ))}
      </div>
    </div>
  );
}
