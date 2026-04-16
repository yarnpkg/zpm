import {createSelector, createSlice, type PayloadAction} from '@reduxjs/toolkit';

import type {LongLivedTaskInfo, TaskEvent, TaskEventState} from '../../generated/daemon-protocol';

import type {RootState}                                    from '../index';

const MAX_INSTANCES = 3;

export interface TaskInstance {
  contextualTaskId: string;
  state: TaskEventState;
  date: number;
}

export interface HistorySliceState {
  events: Array<TaskEvent>;
  runningLongLived: Record<string, LongLivedTaskInfo>;
  /** Number of currently-running instances per task key (workspace:taskName). */
  runningCounts: Record<string, number>;
  loading: boolean;
}

const initialState: HistorySliceState = {
  events: [],
  runningLongLived: {},
  runningCounts: {},
  loading: false,
};

function isRunningState(type: TaskEventState[`type`]): boolean {
  return type === `started` || type === `warm-up` || type === `live` || type === `scheduled`;
}

function computeRunningCounts(events: Array<TaskEvent>): Record<string, number> {
  // Find the latest event per contextual task ID
  const latest = new Map<string, TaskEventState>();
  for (const event of events) {
    latest.set(event.contextualTaskId, event.state);
  }

  // Count running instances per task key
  const counts: Record<string, number> = {};
  for (const [contextualTaskId, state] of latest) {
    if (!isRunningState(state.type)) continue;
    const key = taskKeyFromContextualId(contextualTaskId);
    if (key) counts[key] = (counts[key] ?? 0) + 1;
  }
  return counts;
}

function taskKeyFromContextualId(contextualTaskId: string): string | null {
  const atIdx = contextualTaskId.lastIndexOf(`@`);
  if (atIdx === -1) return null;
  return contextualTaskId.slice(0, atIdx);
}

function longLivedKeyFromTaskKey(taskKey: string): {workspace: string; taskName: string} | null {
  const colonIdx = taskKey.indexOf(`:`);
  if (colonIdx === -1) return null;
  return {workspace: taskKey.slice(0, colonIdx), taskName: taskKey.slice(colonIdx + 1)};
}

export const historySlice = createSlice({
  name: `history`,
  initialState,
  reducers: {
    fetchHistoryStarted(state) {
      state.loading = true;
    },
    fetchHistorySucceeded(state, action: PayloadAction<{
      events: Array<TaskEvent>;
      longLivedTasks: Array<LongLivedTaskInfo>;
    }>) {
      state.events = action.payload.events;
      state.runningLongLived = {};
      for (const task of action.payload.longLivedTasks) {
        if (task.status !== `stopped`) {
          const key = `${task.workspace}:${task.taskName}`;
          state.runningLongLived[key] = task;
        }
      }
      state.runningCounts = computeRunningCounts(action.payload.events);
      state.loading = false;
    },
    fetchHistoryFailed(state) {
      state.loading = false;
    },

    taskStarted(state, action: PayloadAction<{taskId: string; isLongLived: boolean}>) {
      state.events.push({
        date: Date.now(),
        contextualTaskId: action.payload.taskId,
        state: {type: `started`, pid: 0},
      });
      const key = taskKeyFromContextualId(action.payload.taskId);
      if (key) {
        state.runningCounts[key] = (state.runningCounts[key] ?? 0) + 1;
        if (action.payload.isLongLived) {
          const parsed = longLivedKeyFromTaskKey(key);
          if (parsed) {
            state.runningLongLived[key] = {
              workspace: parsed.workspace,
              taskName: parsed.taskName,
              status: {running: {started_at_ms: Date.now(), process_id: null}},
            };
          }
        }
      }
    },
    taskCompleted(state, action: PayloadAction<{taskId: string; exitCode: number; signal: number | null}>) {
      const {taskId, exitCode, signal} = action.payload;
      const eventState: TaskEventState = exitCode === 0
        ? {type: `completed`}
        : {type: `failed`, exit_code: exitCode, signal};
      state.events.push({date: Date.now(), contextualTaskId: taskId, state: eventState});

      const key = taskKeyFromContextualId(taskId);
      if (key) {
        const count = (state.runningCounts[key] ?? 1) - 1;
        if (count > 0) state.runningCounts[key] = count;
        else delete state.runningCounts[key];
        delete state.runningLongLived[key];
      }
    },
    taskCancelled(state, action: PayloadAction<{taskId: string}>) {
      state.events.push({
        date: Date.now(),
        contextualTaskId: action.payload.taskId,
        state: {type: `cancelled`},
      });
      const key = taskKeyFromContextualId(action.payload.taskId);
      if (key) {
        const count = (state.runningCounts[key] ?? 1) - 1;
        if (count > 0) state.runningCounts[key] = count;
        else delete state.runningCounts[key];
        delete state.runningLongLived[key];
      }
    },
    taskWarmUpComplete(state, action: PayloadAction<{taskId: string}>) {
      state.events.push({
        date: Date.now(),
        contextualTaskId: action.payload.taskId,
        state: {type: `live`, pid: 0},
      });
    },

    clearHistory() {
      return initialState;
    },
  },
});

export const {
  fetchHistoryStarted, fetchHistorySucceeded, fetchHistoryFailed,
  taskStarted, taskCompleted, taskCancelled, taskWarmUpComplete,
  clearHistory,
} = historySlice.actions;

// --- Selectors ---

export const selectHistoryEvents = (state: RootState) => state.history.events;
export const selectHistoryLoading = (state: RootState) => state.history.loading;

export const selectHistoryEventsSortedDesc = createSelector(
  selectHistoryEvents,
  events => [...events].sort((a, b) => b.date - a.date),
);

export const selectRunningSet = createSelector(
  (state: RootState) => state.history.runningCounts,
  runningCounts => new Set(Object.keys(runningCounts)),
);

export const selectInstanceMap = createSelector(
  selectHistoryEvents,
  events => {
    // Collect the latest state per contextual task ID
    const latest = new Map<string, {state: TaskEventState; date: number}>();
    for (const event of events) {
      const existing = latest.get(event.contextualTaskId);
      if (!existing || event.date >= existing.date) {
        latest.set(event.contextualTaskId, {state: event.state, date: event.date});
      }
    }

    // Group by task key (workspace:taskName)
    const map = new Map<string, Array<TaskInstance>>();
    for (const [contextualTaskId, info] of latest) {
      const key = taskKeyFromContextualId(contextualTaskId);
      if (!key) continue;

      let list = map.get(key);
      if (!list) {
        list = [];
        map.set(key, list);
      }
      list.push({contextualTaskId, state: info.state, date: info.date});
    }

    // Sort each group by date descending and keep only the last N
    for (const [key, list] of map) {
      list.sort((a, b) => b.date - a.date);
      if (list.length > MAX_INSTANCES) {
        map.set(key, list.slice(0, MAX_INSTANCES));
      }
    }

    return map;
  },
);
