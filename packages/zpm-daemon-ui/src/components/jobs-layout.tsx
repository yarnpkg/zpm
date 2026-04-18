import {useCallback, useMemo}                                                                 from 'react';

import type {DeclaredTaskInfo, TaskEventState}                                                from '../generated/daemon-protocol';
import {useDaemon}                                                                            from '../lib/daemon-context';
import {type FuzzyMatch, fuzzyMatch}                                                          from '../lib/fuzzy-match';
import {useAppDispatch, useAppSelector}                                                       from '../store/hooks';
import {selectIsConnected}                                                                    from '../store/slices/connectionSlice';
import {type TaskInstance, selectInstanceMap, selectRunningSet}                               from '../store/slices/historySlice';
import {setFilter, selectTask, selectJobsFilter, selectActiveTaskKey, selectActiveInstanceId} from '../store/slices/jobsUiSlice';
import {selectDeclaredTasks, selectTaskfileErrors, selectTasksLoading}                        from '../store/slices/tasksSlice';

import {TaskTerminal}                                                                         from './task-terminal';

interface MatchedTask {
  task: DeclaredTaskInfo;
  match: FuzzyMatch | null;
  label: string;
}

function HighlightedText({text, ranges}: {text: string, ranges: Array<[number, number]>}) {
  if (ranges.length === 0)
    return <>{text}</>;

  const parts: Array<React.ReactNode> = [];
  let last = 0;

  for (const [start, end] of ranges) {
    if (start > last)
      parts.push(<span key={`t-${last}`}>{text.slice(last, start)}</span>);

    parts.push(
      <span key={`h-${start}`} className={`text-blue-600 font-semibold`}>
        {text.slice(start, end)}
      </span>,
    );
    last = end;
  }

  if (last < text.length)
    parts.push(<span key={`t-${last}`}>{text.slice(last)}</span>);

  return <>{parts}</>;
}

type TaskStatus = `stopped` | `running`;

function statusDotColor(status: TaskStatus): string {
  switch (status) {
    case `running`: return `bg-green-500`;
    case `stopped`: return `bg-slate-300`;
    default: throw new Error(`Unknown status: ${status satisfies never}`);
  }
}

function instanceBadge(state: TaskEventState): {label: string, className: string} {
  switch (state.type) {
    case `scheduled`: return {label: `Queued`, className: `bg-slate-100 text-slate-600`};
    case `started`: return {label: `Running`, className: `bg-blue-100 text-blue-700`};
    case `warm-up`: return {label: `Warm-up`, className: `bg-yellow-100 text-yellow-700`};
    case `live`: return {label: `Live`, className: `bg-green-100 text-green-700`};
    case `completed`: return {label: `OK`, className: `bg-green-100 text-green-700`};
    case `failed`: return {label: `Fail`, className: `bg-red-100 text-red-700`};
    case `cancelled`: return {label: `Cancel`, className: `bg-slate-100 text-slate-500`};
    default: throw new Error(`Unknown state: ${(state as any).type}`);
  }
}

function formatTime(ms: number): string {
  const d = new Date(ms);
  return d.toLocaleTimeString([], {hour: `2-digit`, minute: `2-digit`, second: `2-digit`});
}

function TaskRow({task, match, label, status, isActive, onRun, onStop, onSelect, instances, activeTaskId, onSelectInstance}: {
  task: DeclaredTaskInfo;
  match: FuzzyMatch | null;
  label: string;
  status: TaskStatus;
  isActive: boolean;
  onRun: () => void;
  onStop: () => void;
  onSelect: () => void;
  instances: Array<TaskInstance>;
  activeTaskId: string | null;
  onSelectInstance: (contextualTaskId: string) => void;
}) {
  const isRunning = status === `running`;
  const canRun = !(task.isLongLived && isRunning);

  const prefixLen = task.workspace.length + 1;
  const adjustedRanges: Array<[number, number]> = [];
  for (const [start, end] of match?.ranges ?? []) {
    const s = Math.max(0, start - prefixLen);
    const e = Math.min(label.length, end - prefixLen);
    if (s < e) {
      adjustedRanges.push([s, e]);
    }
  }

  return (
    <li>
      <div
        className={`group flex cursor-pointer select-none items-center gap-3 px-3 py-1 text-sm text-slate-700 hover:bg-slate-100 ${isActive ? `bg-blue-50` : ``}`}
        onClick={onSelect}
        onDoubleClick={canRun ? onRun : undefined}
      >
        <span className={`inline-block h-1.5 w-1.5 flex-none rounded-full ${statusDotColor(status)}`} />
        <span className={`flex-1 truncate`}>
          <HighlightedText text={label} ranges={adjustedRanges} />
        </span>
        {isRunning ? (
          <button
            type={`button`}
            onClick={e => {
              e.stopPropagation(); onStop();
            }}
            className={`invisible ml-1 flex-none rounded p-0.5 text-red-400 hover:text-red-600 group-hover:visible`}
            title={`Stop ${task.workspace}:${task.taskName}`}
          >
            <svg viewBox={`0 0 16 16`} fill={`currentColor`} className={`h-3.5 w-3.5`}>
              <rect x={`3`} y={`3`} width={`10`} height={`10`} rx={`1`} />
            </svg>
          </button>
        ) : (
          <button
            type={`button`}
            onClick={e => {
              e.stopPropagation(); onRun();
            }}
            className={`invisible ml-1 flex-none rounded p-0.5 text-slate-400 hover:text-blue-600 group-hover:visible`}
            title={`Run ${task.workspace}:${task.taskName}`}
          >
            <svg viewBox={`0 0 16 16`} fill={`currentColor`} className={`h-3.5 w-3.5`}>
              <path d={`M4 2l10 6-10 6V2z`} />
            </svg>
          </button>
        )}
      </div>
      {instances.length > 0 && (
        <ul className={`pb-0.5`}>
          {instances.map(inst => {
            const badge = instanceBadge(inst.state);
            const isInstanceActive = inst.contextualTaskId === activeTaskId;
            return (
              <li key={inst.contextualTaskId}>
                <button
                  type={`button`}
                  onClick={() => onSelectInstance(inst.contextualTaskId)}
                  className={`flex w-full cursor-pointer select-none items-center gap-1.5 py-0.5 pl-8 pr-3 text-xs hover:bg-slate-100 ${isInstanceActive ? `bg-blue-50 text-blue-700` : `text-slate-500`}`}
                >
                  <span className={`inline-block rounded px-1 py-px text-[10px] font-medium leading-tight ${badge.className}`}>
                    {badge.label}
                  </span>
                  <span className={`truncate tabular-nums`}>{formatTime(inst.date)}</span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </li>
  );
}

function taskKey(workspace: string, taskName: string): string {
  return `${workspace}:${taskName}`;
}

export function JobsLayout() {
  const daemon = useDaemon();
  const dispatch = useAppDispatch();

  const isConnected = useAppSelector(selectIsConnected);
  const declaredTasks = useAppSelector(selectDeclaredTasks);
  const taskfileErrors = useAppSelector(selectTaskfileErrors);
  const tasksLoading = useAppSelector(selectTasksLoading);
  const runningSet = useAppSelector(selectRunningSet);
  const instanceMap = useAppSelector(selectInstanceMap);

  const filter = useAppSelector(selectJobsFilter);
  const activeTaskKey = useAppSelector(selectActiveTaskKey);
  const activeInstanceId = useAppSelector(selectActiveInstanceId);

  const grouped = useMemo(() => {
    if (declaredTasks.length === 0 && !tasksLoading) return new Map<string, Array<MatchedTask>>();
    if (declaredTasks.length === 0) return null;

    const matched: Array<MatchedTask> = [];
    const needle = filter.trim();

    for (const task of declaredTasks) {
      const label = `${task.workspace}:${task.taskName}`;
      if (needle === ``) {
        matched.push({task, match: null, label});
      } else {
        const m = fuzzyMatch(needle, label);
        if (m) {
          matched.push({task, match: m, label});
        }
      }
    }

    if (needle !== ``)
      matched.sort((a, b) => (b.match?.score ?? 0) - (a.match?.score ?? 0));

    const groups = new Map<string, Array<MatchedTask>>();
    for (const entry of matched) {
      let group = groups.get(entry.task.workspace);
      if (!group) {
        group = [];
        groups.set(entry.task.workspace, group);
      }
      group.push(entry);
    }

    return groups;
  }, [declaredTasks, tasksLoading, filter]);

  const handleRun = useCallback((task: DeclaredTaskInfo) => {
    if (!daemon) return;
    const key = taskKey(task.workspace, task.taskName);
    const contextId = crypto.randomUUID();
    daemon.pushTasks(
      [{name: task.taskName, args: []}],
      task.workspace,
      contextId,
      {outputSubscription: `fullTree`, statusSubscription: `fullTree`},
    ).then(result => {
      if (result.taskIds.length > 0) {
        dispatch(selectTask({key, instanceId: result.taskIds[0]!}));
      }
    });
  }, [daemon, dispatch]);

  const handleStop = useCallback((task: DeclaredTaskInfo) => {
    if (!daemon) return;
    daemon.stopTask(task.taskName, task.workspace);
  }, [daemon]);

  const handleSelect = useCallback((key: string) => {
    dispatch(selectTask({key}));
  }, [dispatch]);

  const handleSelectInstance = useCallback((key: string, contextualTaskId: string) => {
    dispatch(selectTask({key, instanceId: contextualTaskId}));
  }, [dispatch]);

  const activeTaskIds = useMemo(() => {
    if (!activeTaskKey) return [];

    if (activeInstanceId) return [activeInstanceId];

    const instances = instanceMap.get(activeTaskKey);
    if (!instances || instances.length === 0) return [];

    return instances.map(inst => inst.contextualTaskId).reverse();
  }, [activeTaskKey, activeInstanceId, instanceMap]);

  return (
    <div className={`flex h-full`}>
      <aside className={`flex w-72 flex-col border-r border-slate-200 bg-white`}>
        <div className={`border-b border-slate-200 p-2`}>
          <input
            type={`text`}
            value={filter}
            onChange={e => dispatch(setFilter(e.target.value))}
            placeholder={`Filter tasks…`}
            className={`w-full rounded border border-slate-300 px-2 py-1 text-sm outline-none focus:border-blue-400 focus:ring-1 focus:ring-blue-400`}
          />
        </div>

        <div className={`flex-1 overflow-y-auto`}>
          {!isConnected ? (
            <p className={`p-3 text-xs text-slate-400`}>Waiting for connection…</p>
          ) : tasksLoading ? (
            <p className={`p-3 text-xs text-slate-400`}>Loading tasks…</p>
          ) : grouped && grouped.size === 0 && taskfileErrors.length === 0 ? (
            <p className={`p-3 text-xs text-slate-400`}>No matching tasks.</p>
          ) : grouped ? (
            <>
              {taskfileErrors.length > 0 && (
                <div className={`space-y-2 p-2`}>
                  {taskfileErrors.map(error => (
                    <div key={error.workspace} className={`rounded border border-red-200 bg-red-50 p-2`}>
                      <p className={`text-xs font-semibold text-red-700`}>{error.workspace}</p>
                      <p className={`mt-0.5 text-xs text-red-600 whitespace-pre-wrap`}>{error.message}</p>
                    </div>
                  ))}
                </div>
              )}
              {grouped.size > 0 && (
                <ul className={`space-y-4`}>
                  {[...grouped.entries()].map(([workspace, entries]) => (
                    <li key={workspace}>
                      <p className={`sticky top-0 bg-slate-50 p-2 text-xs font-semibold text-slate-500`}>
                        {workspace}
                      </p>
                      <ul>
                        {entries.map(entry => {
                          const key = taskKey(entry.task.workspace, entry.task.taskName);
                          const status: TaskStatus = runningSet.has(key) ? `running` : `stopped`;
                          const instances = entry.task.isLongLived ? [] : (instanceMap.get(key) ?? []);
                          return (
                            <TaskRow
                              key={entry.label}
                              task={entry.task}
                              match={entry.match}
                              label={entry.task.taskName}
                              status={status}
                              isActive={activeTaskKey === key && activeInstanceId === null}
                              onRun={() => handleRun(entry.task)}
                              onStop={() => handleStop(entry.task)}
                              onSelect={() => handleSelect(key)}
                              instances={instances}
                              activeTaskId={activeInstanceId}
                              onSelectInstance={id => handleSelectInstance(key, id)}
                            />
                          );
                        })}
                      </ul>
                    </li>
                  ))}
                </ul>
              )}
            </>
          ) : null}
        </div>
      </aside>

      <div className={`relative flex-1 bg-slate-900`}>
        {activeTaskIds.length > 0 ? (
          <div className={`absolute inset-0 p-2`}>
            <TaskTerminal taskIds={activeTaskIds} />
          </div>
        ) : (
          <div className={`flex h-full items-center justify-center text-sm text-slate-500`}>
            Select a task to run
          </div>
        )}
      </div>
    </div>
  );
}
