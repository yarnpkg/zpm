import {createSlice, type PayloadAction}      from '@reduxjs/toolkit';

import type {DeclaredTaskInfo, TaskfileError} from '../../generated/daemon-protocol';
import type {RootState}                       from '../index';

export interface TasksSliceState {
  declaredTasks: Array<DeclaredTaskInfo>;
  taskfileErrors: Array<TaskfileError>;
  loading: boolean;
}

const initialState: TasksSliceState = {
  declaredTasks: [],
  taskfileErrors: [],
  loading: false,
};

export const tasksSlice = createSlice({
  name: `tasks`,
  initialState,
  reducers: {
    fetchTasksStarted(state) {
      state.loading = true;
    },
    fetchTasksSucceeded(state, action: PayloadAction<{tasks: Array<DeclaredTaskInfo>, errors: Array<TaskfileError>}>) {
      state.declaredTasks = action.payload.tasks;
      state.taskfileErrors = action.payload.errors;
      state.loading = false;
    },
    fetchTasksFailed(state) {
      state.loading = false;
    },
    clearTasks() {
      return initialState;
    },
  },
});

export const {fetchTasksStarted, fetchTasksSucceeded, fetchTasksFailed, clearTasks} = tasksSlice.actions;

export const selectDeclaredTasks = (state: RootState) => state.tasks.declaredTasks;
export const selectTaskfileErrors = (state: RootState) => state.tasks.taskfileErrors;
export const selectTasksLoading = (state: RootState) => state.tasks.loading;
