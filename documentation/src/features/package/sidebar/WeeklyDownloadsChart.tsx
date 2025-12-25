import 'preact/compat';
import {AreaChart, Area, ResponsiveContainer, Tooltip} from 'recharts';
import {useWeeklyDownloads}                            from '@/api/package';
import DownloadIcon                                    from '@/assets/svg/download-icon.svg?react';

const formatter = Intl.NumberFormat(`en-US`, {
  compactDisplay: `long`,
  notation: `standard`,
});

function formatNumber(number: number) {
  return formatter.format(number).replaceAll(`,`, ` `);
}

interface WeeklyDownloadsChartProps {
  packageName: string;
}

const MONTHS = [
  `January`,
  `February`,
  `March`,
  `April`,
  `May`,
  `June`,
  `July`,
  `August`,
  `September`,
  `October`,
  `November`,
  `December`,
];

const CustomTooltip = ({
  active,
  payload,
}: {
  label: string;
  payload:
    | [{payload: {day: string, downloads: number}, value: number}]
    | [];
  active: boolean;
}) => {
  if (!active || !payload || !payload.length) return null;

  const date = payload[0].payload.day; // this is your `day`
  const count = payload[0].value; // downloads for that day

  const [_, month, day] = date.split(`-`);

  return (
    <div class={`p-px bg-linear-to-b from-white/15 to-white/5 rounded-md`}>
      <div class={`text-sm text-white flex gap-y-2 flex-col font-medium bg-linear-to-b from-[#181A1F]/80 to-[#0D0F14]/80 px-4 py-2 rounded-md`}>
        <span class={`text-center`}>
          {MONTHS[Number(month) - 1]} {day}
        </span>
        <div class={`flex !items-center gap-x-2 text-lg font-medium text-nowrap`}>
          <DownloadIcon class={`size-5`} />
          <span class={`leading-0`}>{formatNumber(count)}</span>
        </div>
      </div>
    </div>
  );
};

function Dot(props: {cx: number, cy: number}) {
  const {cx, cy} = props;

  return (
    <g
      style={{
        filter: `drop-shadow(0 0 10px rgba(255,255,255,0.3))`,
      }}
    >
      <circle cx={cx} cy={cy} r={10} class={`fill-blue-500/60`} />
      {/* Outer shadow circle */}
      <circle
        cx={cx}
        cy={cy}
        r={6}
        fill={`var(--color-blue-500)`}
        style={{
          filter: `drop-shadow(0 0 10px var(--color-blue-500))`,
        }}
      />
      {/* Inner white circle */}
      <circle cx={cx} cy={cy} r={4} fill={`#ffffff`} />
    </g>
  );
}

export default function WeeklyDownloadsChart({
  packageName,
}: WeeklyDownloadsChartProps) {
  const {data} = useWeeklyDownloads(packageName);

  if (!data) return null;

  const total = data.downloads.reduce(
    (prev: number, current: {downloads: number}) => prev + current.downloads,
    0,
  );
  const chartData = data.downloads;

  return (
    <div class={`p-px bg-linear-to-b from-white/15 to-white/5 rounded-2xl`}>
      <div className={`flex flex-col gap-y-4 w-full p-4 rounded-2xl bg-linear-to-b from-[#181A1F] to-[#181A1F] font-montserrat`}>
        <div class={`flex justify-between items-center gap-x-2 gap-y-1 flex-wrap`}>
          <div class={`flex gap-x-3 items-center`}>
            <DownloadIcon class={`stroke-[1.3]`} />
            <span class={`leading-[1.4] font-medium text-white text-wrap`}>
              Weekly Downloads
            </span>
          </div>
          <span class={`text-[32px] text-white font-medium leading-[1.2] text-nowrap`}>
            {formatNumber(total)}
          </span>
        </div>
        <div class={`w-full h-15 flex justify-center items-center`}>
          <ResponsiveContainer width={`100%`} height={`100%`}>
            <AreaChart data={chartData}>
              <defs>
                <linearGradient id={`areaGradient`} x1={`0`} y1={`1`} x2={`0`} y2={`0`}>
                  <stop offset={`0%`} stopColor={`transparent`} stopOpacity={0} />
                  <stop
                    offset={`100%`}
                    stopColor={`var(--color-blue-500)`}
                    stopOpacity={0.3}
                  />
                </linearGradient>
              </defs>
              <Tooltip
                // @ts-expect-error: TOOD: Fix this
                content={<CustomTooltip />}
                cursor={{
                  stroke: `rgba(255,255,255,0.6)`,
                  strokeWidth: 2,
                  opacity: 0.3,
                }}
              />
              <Area
                type={`monotone`}
                dataKey={`downloads`}
                stroke={`var(--color-blue-500)`}
                fill={`url(#areaGradient)`}
                strokeWidth={2}
                // @ts-expect-error: TOOD: Fix this
                activeDot={<Dot />}
                style={{
                  filter: `drop-shadow(0 0 7px rgba(255, 255, 255, 0.4))`,
                }}
              />
            </AreaChart>
          </ResponsiveContainer>
        </div>
      </div>
    </div>
  );
}
