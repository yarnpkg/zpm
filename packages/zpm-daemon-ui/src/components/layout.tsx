import {Link, Outlet}                                                     from '@tanstack/react-router';

import {useDaemon}                                                        from '../lib/daemon-context';
import {getDaemonUrl}                                                     from '../lib/daemon';
import {useAppSelector}                                                   from '../store/hooks';
import {selectConnectionError, selectConnectionStatus, selectIsConnected} from '../store/slices/connectionSlice';
import {selectMeta}                                                       from '../store/slices/metaSlice';

const NAV_ITEMS = [
  {to: `/`, label: `Dashboard`},
  {to: `/jobs`, label: `Jobs`},
  {to: `/history`, label: `History`},
] as const;

function ConnectionBadge() {
  const state = useAppSelector(selectConnectionStatus);

  const colors: Record<string, string> = {
    connected: `bg-green-500`,
    connecting: `bg-yellow-500`,
    disconnected: `bg-red-500`,
    rejected: `bg-red-500`,
  };

  const labels: Record<string, string> = {
    connected: `Connected`,
    connecting: `Connecting…`,
    disconnected: `Disconnected`,
    rejected: `Rejected`,
  };

  return (
    <div className={`flex items-center gap-2 text-xs text-slate-500`}>
      <span className={`inline-block h-2 w-2 rounded-full ${colors[state]}`} />
      {labels[state]}
    </div>
  );
}

function ConnectionError() {
  const error = useAppSelector(selectConnectionError);

  if (!error)
    return null;

  return (
    <div className={`border-b border-red-200 bg-red-50 px-4 py-3 text-sm text-red-800`}>
      <span className={`font-medium`}>Connection error:</span> {error}
    </div>
  );
}

export function Layout() {
  const daemonUrl = getDaemonUrl();
  const daemon = useDaemon();
  const isConnected = useAppSelector(selectIsConnected);
  const meta = useAppSelector(selectMeta);

  function handleStopDaemon() {
    daemon?.shutdown();
  }

  return (
    <div className={`flex min-h-screen`}>
      <nav className={`flex w-56 flex-col border-r border-slate-200 bg-white`}>
        <div className={`border-b border-slate-200 p-4`}>
          <h1 className={`text-lg font-semibold text-slate-900`}>Yarn Project Panel</h1>
          <div className={`mt-2`}>
            <ConnectionBadge />
          </div>
        </div>

        <ul className={`flex-1 p-2`}>
          {NAV_ITEMS.map(({to, label}) => (
            <li key={to}>
              <Link
                to={to}
                className={`block rounded px-3 py-2 text-sm text-slate-700 hover:bg-slate-100`}
                activeProps={{className: `block rounded px-3 py-2 text-sm font-medium text-slate-900 bg-slate-100`}}
              >
                {label}
              </Link>
            </li>
          ))}
        </ul>

        <div className={`border-t border-slate-200 p-3 space-y-0.5 text-xs text-slate-400`}>
          <p className={`truncate`} title={daemonUrl}>{daemonUrl}</p>
          {meta ? (
            <>
              <p>{meta.version}</p>
              <p className={`truncate`} title={meta.cwd}>{meta.cwd}</p>
            </>
          ) : null}
        </div>

        <div className={`border-t border-slate-200 p-2`}>
          <button
            type={`button`}
            onClick={handleStopDaemon}
            disabled={!isConnected}
            className={`flex w-full items-center gap-2 rounded px-3 py-2 text-sm text-red-600 hover:bg-red-50 disabled:cursor-not-allowed disabled:text-slate-400 disabled:hover:bg-transparent`}
          >
            <svg viewBox={`0 0 16 16`} fill={`currentColor`} className={`h-4 w-4`}>
              <rect x={`2`} y={`2`} width={`12`} height={`12`} rx={`2`} />
            </svg>
            Stop Daemon
          </button>
        </div>
      </nav>

      <div className={`flex flex-1 flex-col`}>
        <ConnectionError />
        <main className={`flex-1 bg-slate-50`}>
          <Outlet />
        </main>
      </div>
    </div>
  );
}
