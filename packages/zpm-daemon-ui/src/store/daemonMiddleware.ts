import type {DaemonNotification}                                                                                                                     from '../generated/daemon-protocol';
import type {DaemonConnection}                                                                                                                       from '../lib/daemon';

import type {AppDispatch, RootState}                                                                                                                 from './index';
import {setConnectionError, setConnectionStatus}                                                                                                     from './slices/connectionSlice';
import {clearHistory, fetchHistoryFailed, fetchHistoryStarted, fetchHistorySucceeded, taskCancelled, taskCompleted, taskStarted, taskWarmUpComplete} from './slices/historySlice';
import {clearMeta, fetchMetaFailed, fetchMetaStarted, fetchMetaSucceeded}                                                                            from './slices/metaSlice';
import {clearStats, fetchStatsFailed, fetchStatsStarted, fetchStatsSucceeded}                                                                        from './slices/statsSlice';
import {clearTasks, fetchTasksFailed, fetchTasksStarted, fetchTasksSucceeded}                                                                        from './slices/tasksSlice';

let statsIntervalId: number | null = null;

function startStatsPolling(daemon: DaemonConnection, dispatch: AppDispatch) {
  stopStatsPolling();
  statsIntervalId = window.setInterval(async () => {
    try {
      const stats = await daemon.getStats();
      dispatch(fetchStatsSucceeded(stats));
    } catch {
      // Silently ignore — disconnect will be handled by connection state listener.
    }
  }, 5_000);
}

function stopStatsPolling() {
  if (statsIntervalId !== null) {
    window.clearInterval(statsIntervalId);
    statsIntervalId = null;
  }
}

async function initialDataLoad(daemon: DaemonConnection, dispatch: AppDispatch, getState: () => RootState) {
  dispatch(fetchMetaStarted());
  dispatch(fetchTasksStarted());
  dispatch(fetchHistoryStarted());
  dispatch(fetchStatsStarted());

  const results = await Promise.allSettled([
    daemon.getMeta(),
    daemon.listDeclaredTasks(),
    Promise.all([daemon.getTaskHistory(), daemon.listLongLivedTasks()]),
    daemon.getStats(),
  ]);

  if (results[0].status === `fulfilled`)
    dispatch(fetchMetaSucceeded(results[0].value));
  else
    dispatch(fetchMetaFailed());


  if (results[1].status === `fulfilled`)
    dispatch(fetchTasksSucceeded(results[1].value));
  else
    dispatch(fetchTasksFailed());


  if (results[2].status === `fulfilled`) {
    const [events, longLivedTasks] = results[2].value;
    dispatch(fetchHistorySucceeded({events, longLivedTasks}));
  } else {
    dispatch(fetchHistoryFailed());
  }

  if (results[3].status === `fulfilled`)
    dispatch(fetchStatsSucceeded(results[3].value));
  else
    dispatch(fetchStatsFailed());


  startStatsPolling(daemon, dispatch);
}

function isTaskLongLived(taskId: string, getState: () => RootState): boolean {
  const key = parseTaskKey(taskId);
  if (!key) return false;

  const colonIdx = key.indexOf(`:`);
  if (colonIdx === -1) return false;

  const workspace = key.slice(0, colonIdx);
  const taskName = key.slice(colonIdx + 1);
  const declaredTasks = getState().tasks.declaredTasks;

  return declaredTasks.some(t => t.workspace === workspace && t.taskName === taskName && t.isLongLived);
}

function parseTaskKey(contextualTaskId: string): string | null {
  const atIdx = contextualTaskId.lastIndexOf(`@`);
  if (atIdx === -1) return null;
  return contextualTaskId.slice(0, atIdx);
}

/**
 * Bridges a DaemonConnection into the Redux store.
 * Call once when the connection is created. Returns a cleanup function.
 */
export function bindDaemonToStore(
  daemon: DaemonConnection,
  dispatch: AppDispatch,
  getState: () => RootState,
): () => void {
  // Sync initial connection state.
  dispatch(setConnectionStatus(daemon.getState()));
  if (daemon.getConnectionError())
    dispatch(setConnectionError(daemon.getConnectionError()));


  const unsubState = daemon.onStateChange(state => {
    dispatch(setConnectionStatus(state));

    if (state === `connected`)
      initialDataLoad(daemon, dispatch, getState);


    if (state === `disconnected` || state === `rejected`) {
      stopStatsPolling();
      dispatch(clearMeta());
      dispatch(clearTasks());
      dispatch(clearStats());
      dispatch(clearHistory());

      if (state === `rejected`) {
        dispatch(setConnectionError(daemon.getConnectionError()));
      }
    }
  });

  const unsubNotifications = daemon.onNotification((notification: DaemonNotification) => {
    switch (notification.type) {
      case `taskStarted`:
        dispatch(taskStarted({
          taskId: notification.taskId,
          isLongLived: isTaskLongLived(notification.taskId, getState),
        }));
        break;
      case `taskCompleted`:
        dispatch(taskCompleted({
          taskId: notification.taskId,
          exitCode: notification.exitCode,
          signal: notification.signal,
        }));
        break;
      case `taskCancelled`:
        dispatch(taskCancelled({taskId: notification.taskId}));
        break;
      case `taskWarmUpComplete`:
        dispatch(taskWarmUpComplete({taskId: notification.taskId}));
        break;
      case `declaredTasksChanged`:
        dispatch(fetchTasksSucceeded({tasks: notification.tasks, errors: notification.errors}));
        break;
      // taskOutputLine is handled directly by TaskTerminal, not stored in Redux.
    }
  });

  // If already connected, load immediately.
  if (daemon.getState() === `connected`)
    initialDataLoad(daemon, dispatch, getState);


  return () => {
    stopStatsPolling();
    unsubState();
    unsubNotifications();
  };
}
