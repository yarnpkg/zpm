import {createSlice, type PayloadAction} from '@reduxjs/toolkit';

import type {DaemonMeta}                               from '../../generated/daemon-protocol';

import type {RootState}                                from '../index';

export interface MetaSliceState {
  data: DaemonMeta | null;
  loading: boolean;
}

const initialState: MetaSliceState = {
  data: null,
  loading: false,
};

export const metaSlice = createSlice({
  name: `meta`,
  initialState,
  reducers: {
    fetchMetaStarted(state) {
      state.loading = true;
    },
    fetchMetaSucceeded(state, action: PayloadAction<DaemonMeta>) {
      state.data = action.payload;
      state.loading = false;
    },
    fetchMetaFailed(state) {
      state.loading = false;
    },
    clearMeta() {
      return initialState;
    },
  },
});

export const {fetchMetaStarted, fetchMetaSucceeded, fetchMetaFailed, clearMeta} = metaSlice.actions;

export const selectMeta = (state: RootState) => state.meta.data;
export const selectMetaLoading = (state: RootState) => state.meta.loading;
