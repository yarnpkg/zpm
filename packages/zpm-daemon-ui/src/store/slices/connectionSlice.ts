import {createSlice, type PayloadAction} from '@reduxjs/toolkit';

import type {ConnectionState}            from '../../lib/daemon';
import type {RootState}                  from '../index';

export interface ConnectionSliceState {
  status: ConnectionState;
  error: string | null;
}

const initialState: ConnectionSliceState = {
  status: `connecting`,
  error: null,
};

export const connectionSlice = createSlice({
  name: `connection`,
  initialState,
  reducers: {
    setConnectionStatus(state, action: PayloadAction<ConnectionState>) {
      state.status = action.payload;
    },
    setConnectionError(state, action: PayloadAction<string | null>) {
      state.error = action.payload;
    },
  },
});

export const {setConnectionStatus, setConnectionError} = connectionSlice.actions;

export const selectConnectionStatus = (state: RootState) => state.connection.status;
export const selectConnectionError = (state: RootState) => state.connection.error;
export const selectIsConnected = (state: RootState) => state.connection.status === `connected`;
