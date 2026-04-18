import {RouterProvider} from '@tanstack/react-router';
import {createRoot}     from 'react-dom/client';
import {Provider}       from 'react-redux';
import React            from 'react';

import {DaemonProvider} from './lib/daemon-context';
import {router}         from './router';
import {store}          from './store';
import './styles.css';

const rootElement = document.getElementById(`root`);

if (rootElement === null)
  throw new Error(`Missing #root element`);


createRoot(rootElement).render(
  <React.StrictMode>
    <Provider store={store}>
      <DaemonProvider>
        <RouterProvider router={router} />
      </DaemonProvider>
    </Provider>
  </React.StrictMode>,
);
