import {createSlice, type PayloadAction} from '@reduxjs/toolkit';

import type {RootState}                  from '../index';

export interface ReadmeSliceState {
  content: string | null;
  loading: boolean;
}

const initialState: ReadmeSliceState = {
  content: null,
  loading: false,
};

export const readmeSlice = createSlice({
  name: `readme`,
  initialState,
  reducers: {
    fetchReadmeStarted(state) {
      state.loading = true;
    },
    fetchReadmeSucceeded(state, action: PayloadAction<{content: string | null}>) {
      state.content = action.payload.content;
      state.loading = false;
    },
    fetchReadmeFailed(state) {
      state.loading = false;
    },
    clearReadme() {
      return initialState;
    },
  },
});

export const {
  fetchReadmeStarted, fetchReadmeSucceeded, fetchReadmeFailed,
  clearReadme,
} = readmeSlice.actions;

export const selectReadmeContent = (state: RootState) => state.readme.content;
export const selectReadmeLoading = (state: RootState) => state.readme.loading;
