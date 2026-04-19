import {useState}                                            from 'react';

import type {TaskEventState}                                 from '../generated/daemon-protocol';
import {useAppSelector}                                      from '../store/hooks';
import {selectIsConnected}                                   from '../store/slices/connectionSlice';
import {selectHistoryEventsSortedDesc, selectHistoryLoading} from '../store/slices/historySlice';

function stateBadge(state: TaskEventState): {label: string, className: string} {
  switch (state.type) {
    case `scheduled`:
      return {label: `Scheduled`, className: `bg-slate-100 text-slate-700`};
    case `started`:
      return {label: `Started (PID ${state.pid})`, className: `bg-blue-100 text-blue-800`};
    case `warm-up`:
      return {label: `Warm-up (PID ${state.pid})`, className: `bg-yellow-100 text-yellow-800`};
    case `live`:
      return {label: `Live (PID ${state.pid})`, className: `bg-green-100 text-green-800`};
    case `completed`:
      return {label: `Completed`, className: `bg-green-100 text-green-800`};
    case `failed`: {
      const detail = state.exit_code !== null ? ` (exit ${state.exit_code})` : ``;
      return {label: `Failed${detail}`, className: `bg-red-100 text-red-800`};
    }
    case `cancelled`:
      return {label: `Cancelled`, className: `bg-slate-100 text-slate-600`};
    default:
      throw new Error(`Unknown state: ${(state as any).type}`);
  }
}

export function HistoryRoute() {
  const isConnected = useAppSelector(selectIsConnected);
  const loading = useAppSelector(selectHistoryLoading);
  const events = useAppSelector(selectHistoryEventsSortedDesc);

  const [hoveredInstance, setHoveredInstance] = useState<string | null>(null);

  return (
    <div className={`p-8`}>
      <h2 className={`text-2xl font-semibold text-slate-900`}>Task History</h2>

      {!isConnected ? (
        <p className={`mt-6 rounded border border-yellow-300 bg-yellow-50 p-3 text-yellow-800`}>
          Waiting for daemon connection…
        </p>
      ) : null}

      {loading && isConnected ? (
        <p className={`mt-6 text-slate-700`}>Loading history…</p>
      ) : null}

      {!loading && events.length === 0 && isConnected ? (
        <p className={`mt-6 text-slate-600`}>No task events recorded.</p>
      ) : null}

      {events.length > 0 ? (
        <ul
          className={`mt-6 divide-y divide-slate-200 rounded border border-slate-200 bg-white`}
          onMouseLeave={() => setHoveredInstance(null)}
        >
          {events.map((event, i) => {
            const badge = stateBadge(event.state);
            const time = new Date(event.date).toLocaleString();
            const isDimmed = hoveredInstance !== null && hoveredInstance !== event.contextualTaskId;
            return (
              <li
                key={`${event.contextualTaskId}-${event.date}-${i}`}
                className={`flex select-none items-center gap-3 p-3 transition-opacity ${isDimmed ? `opacity-30` : ``}`}
                onMouseEnter={() => setHoveredInstance(event.contextualTaskId)}
              >
                <span className={`inline-block rounded px-2 py-0.5 text-xs font-medium ${badge.className}`}>
                  {badge.label}
                </span>
                <span className={`flex-1 truncate text-sm font-medium text-slate-900`}>
                  {event.contextualTaskId}
                </span>
                <span className={`tabular-nums text-xs text-slate-400`}>{time}</span>
              </li>
            );
          })}
        </ul>
      ) : null}
    </div>
  );
}
