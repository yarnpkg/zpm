import {createContext, useContext, useEffect, useState} from 'react';

import {store}                                         from '../store';
import {bindDaemonToStore}                             from '../store/daemonMiddleware';

import {DaemonConnection, getAuthToken, getDaemonUrl}  from './daemon';

const DaemonContext = createContext<DaemonConnection | null>(null);

export function DaemonProvider({children}: {children: React.ReactNode}) {
  const url = getDaemonUrl();
  const token = getAuthToken();
  const [connection, setConnection] = useState<DaemonConnection | null>(null);

  useEffect(() => {
    const conn = new DaemonConnection(url, token);
    setConnection(conn);

    const unbind = bindDaemonToStore(conn, store.dispatch, store.getState);

    return () => {
      unbind();
      conn.dispose();
      setConnection(null);
    };
  }, [url, token]);

  return (
    <DaemonContext.Provider value={connection}>
      {children}
    </DaemonContext.Provider>
  );
}

export function useDaemon(): DaemonConnection | null {
  return useContext(DaemonContext);
}
