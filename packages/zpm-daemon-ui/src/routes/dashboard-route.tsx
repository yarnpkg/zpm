import {useAppSelector}                                from '../store/hooks';
import {selectIsConnected}                             from '../store/slices/connectionSlice';
import {selectMeta}                                    from '../store/slices/metaSlice';
import {selectStats}                                   from '../store/slices/statsSlice';

export function DashboardRoute() {
  const isConnected = useAppSelector(selectIsConnected);
  const meta = useAppSelector(selectMeta);
  const stats = useAppSelector(selectStats);

  return (
    <div className="p-8">
      <h2 className="text-2xl font-semibold text-slate-900">Dashboard</h2>

      {!isConnected ? (
        <p className="mt-6 rounded border border-yellow-300 bg-yellow-50 p-3 text-yellow-800">
          Waiting for daemon connection…
        </p>
      ) : null}

      {meta ? (
        <div className="mt-6 rounded border border-slate-200 bg-white p-4">
          <h3 className="text-sm font-medium text-slate-500">Daemon Version</h3>
          <p className="mt-1 text-lg font-semibold text-slate-900">{meta.version}</p>
        </div>
      ) : null}

      {stats ? (
        <div className="mt-6">
          <h3 className="text-sm font-medium text-slate-500">Internal Stats</h3>
          <div className="mt-2 grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-5">
            <StatCard label="Tasks" value={stats.tasksCount} />
            <StatCard label="Prepared" value={stats.preparedCount} />
            <StatCard label="Subtasks" value={stats.subtasksCount} />
            <StatCard label="Output Buffers" value={stats.outputBufferCount} />
            <StatCard label="Closed Tasks" value={stats.closedTasksCount} />
          </div>
        </div>
      ) : null}
    </div>
  );
}

function StatCard({label, value}: {label: string; value: number}) {
  return (
    <div className="rounded border border-slate-200 bg-white p-3">
      <p className="text-xs text-slate-500">{label}</p>
      <p className="mt-1 text-2xl font-semibold text-slate-900">{value}</p>
    </div>
  );
}
