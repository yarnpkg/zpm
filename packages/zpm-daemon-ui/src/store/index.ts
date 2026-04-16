import {configureStore} from '@reduxjs/toolkit';

import {connectionSlice} from './slices/connectionSlice';
import {historySlice}    from './slices/historySlice';
import {jobsUiSlice}     from './slices/jobsUiSlice';
import {metaSlice}       from './slices/metaSlice';
import {statsSlice}      from './slices/statsSlice';
import {tasksSlice}      from './slices/tasksSlice';

export const store = configureStore({
  reducer: {
    connection: connectionSlice.reducer,
    meta: metaSlice.reducer,
    tasks: tasksSlice.reducer,
    stats: statsSlice.reducer,
    history: historySlice.reducer,
    jobsUi: jobsUiSlice.reducer,
  },
  middleware: getDefaultMiddleware =>
    getDefaultMiddleware({
      serializableCheck: false,
    }),
});

export type RootState = ReturnType<typeof store.getState>;
export type AppDispatch = typeof store.dispatch;
