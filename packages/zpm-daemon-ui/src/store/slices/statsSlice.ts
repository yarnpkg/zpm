import {createSlice, type PayloadAction} from '@reduxjs/toolkit';

import type {RootState}                  from '../index';

export interface DaemonStats {
  tasksCount: number;
  preparedCount: number;
  subtasksCount: number;
  outputBufferCount: number;
  closedTasksCount: number;
  watchedFilesCount: number;
}

export interface StatsSliceState {
  data: DaemonStats | null;
  loading: boolean;
}

const initialState: StatsSliceState = {
  data: null,
  loading: false,
};

export const statsSlice = createSlice({
  name: `stats`,
  initialState,
  reducers: {
    fetchStatsStarted(state) {
      state.loading = true;
    },
    fetchStatsSucceeded(state, action: PayloadAction<DaemonStats>) {
      state.data = action.payload;
      state.loading = false;
    },
    fetchStatsFailed(state) {
      state.loading = false;
    },
    clearStats() {
      return initialState;
    },
  },
});

export const {fetchStatsStarted, fetchStatsSucceeded, fetchStatsFailed, clearStats} = statsSlice.actions;

export const selectStats = (state: RootState) => state.stats.data;
export const selectStatsLoading = (state: RootState) => state.stats.loading;
