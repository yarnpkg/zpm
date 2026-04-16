import {createSlice, type PayloadAction} from '@reduxjs/toolkit';

import type {DeclaredTaskInfo}                         from '../../generated/daemon-protocol';

import type {RootState}                                from '../index';

export interface TasksSliceState {
  declaredTasks: Array<DeclaredTaskInfo>;
  loading: boolean;
}

const initialState: TasksSliceState = {
  declaredTasks: [],
  loading: false,
};

export const tasksSlice = createSlice({
  name: `tasks`,
  initialState,
  reducers: {
    fetchTasksStarted(state) {
      state.loading = true;
    },
    fetchTasksSucceeded(state, action: PayloadAction<Array<DeclaredTaskInfo>>) {
      state.declaredTasks = action.payload;
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
export const selectTasksLoading = (state: RootState) => state.tasks.loading;
