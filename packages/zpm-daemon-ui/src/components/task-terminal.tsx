import '@xterm/xterm/css/xterm.css';
import {FitAddon}                from '@xterm/addon-fit';
import {Terminal}                from '@xterm/xterm';
import {useEffect, useRef}       from 'react';

import type {DaemonNotification} from '../generated/daemon-protocol';
import {useDaemon}               from '../lib/daemon-context';

export function TaskTerminal({taskIds}: {taskIds: Array<string>}) {
  const daemon = useDaemon();
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return undefined;

    const term = new Terminal({
      convertEol: true,
      fontSize: 13,
      fontFamily: `'Menlo', 'Monaco', 'Courier New', monospace`,
      theme: {
        background: `#0f172a`,
        foreground: `#e2e8f0`,
        cursor: `#e2e8f0`,
      },
      scrollback: 10_000,
    });

    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(container);
    fitAddon.fit();

    termRef.current = term;

    const resizeObserver = new ResizeObserver(() => {
      fitAddon.fit();
    });
    resizeObserver.observe(container);

    return () => {
      resizeObserver.disconnect();
      term.dispose();
      termRef.current = null;
    };
  }, []);

  // Clear the terminal and load buffered output when task IDs change.
  useEffect(() => {
    const term = termRef.current;
    if (!term || !daemon || taskIds.length === 0) return undefined;

    term.clear();
    term.reset();

    let cancelled = false;

    for (const taskId of taskIds) {
      daemon.getTaskOutput(taskId).then(lines => {
        if (cancelled) return;
        for (const line of lines) {
          term.writeln(line.line);
        }
      }).catch(() => {
        // Task output may not be available yet.
      });
    }

    return () => {
      cancelled = true;
    };
  }, [daemon, taskIds]);

  // Subscribe to live output notifications for the given task IDs.
  useEffect(() => {
    if (!daemon || taskIds.length === 0) return undefined;

    const taskIdSet = new Set(taskIds);

    const unsubscribe = daemon.onNotification((notification: DaemonNotification) => {
      if (notification.type === `taskOutputLine` && taskIdSet.has(notification.taskId)) {
        termRef.current?.writeln(notification.line);
      }
    });

    return unsubscribe;
  }, [daemon, taskIds]);

  return (
    <div ref={containerRef} className={`h-full w-full`} />
  );
}
