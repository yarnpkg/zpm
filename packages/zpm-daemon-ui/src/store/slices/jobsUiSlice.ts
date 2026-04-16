import {createSlice, type PayloadAction} from '@reduxjs/toolkit';

import type {RootState}                                from '../index';

export interface JobsUiSliceState {
  filter: string;
  activeTaskKey: string | null;
  activeInstanceId: string | null;
}

const initialState: JobsUiSliceState = {
  filter: ``,
  activeTaskKey: null,
  activeInstanceId: null,
};

export const jobsUiSlice = createSlice({
  name: `jobsUi`,
  initialState,
  reducers: {
    setFilter(state, action: PayloadAction<string>) {
      state.filter = action.payload;
    },
    selectTask(state, action: PayloadAction<{key: string; instanceId?: string | null}>) {
      state.activeTaskKey = action.payload.key;
      state.activeInstanceId = action.payload.instanceId ?? null;
    },
  },
});

export const {setFilter, selectTask} = jobsUiSlice.actions;

export const selectJobsFilter = (state: RootState) => state.jobsUi.filter;
export const selectActiveTaskKey = (state: RootState) => state.jobsUi.activeTaskKey;
export const selectActiveInstanceId = (state: RootState) => state.jobsUi.activeInstanceId;
