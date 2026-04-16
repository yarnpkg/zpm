import type {
  DaemonMeta,
  DaemonMessage,
  DaemonNotification,
  DaemonRequest,
  DaemonRequestEnvelope,
  DaemonResponse,
  DeclaredTaskInfo,
  LongLivedTaskInfo,
  BufferedOutputLine,
  SubscriptionScope,
  TaskEvent,
} from '../generated/daemon-protocol';

declare const __DAEMON_PORT__: string;
declare const __DAEMON_TOKEN__: string;

const REQUEST_TIMEOUT_MS = 5000;
const RECONNECT_DELAY_MS = 2000;
const DEFAULT_PORT = 12197;
const SESSION_STORAGE_TOKEN_KEY = `daemon-token`;

export type ConnectionState = `connecting` | `connected` | `disconnected` | `rejected`;

export type ConnectionStateListener = (state: ConnectionState) => void;
export type NotificationListener = (notification: DaemonNotification) => void;

function getTokenFromQueryString(): string | null {
  const search = new URLSearchParams(window.location.search);
  return search.get(`token`);
}

function getDaemonUrlFromQueryString(): string | null {
  const search = new URLSearchParams(window.location.search);
  return search.get(`daemon`);
}

export function getDaemonUrl(): string {
  // Priority: ?daemon= query param > same origin (when served by daemon) > DAEMON_PORT env > default
  const fromQs = getDaemonUrlFromQueryString();
  if (fromQs)
    return fromQs;

  // When served by the daemon itself (detected by ?token= param), use the same host:port
  if (getTokenFromQueryString()) {
    return `ws://${window.location.host}`;
  }

  const port = __DAEMON_PORT__ !== `` ? __DAEMON_PORT__ : String(DEFAULT_PORT);
  return `ws://127.0.0.1:${port}`;
}

/**
 * Returns the auth token to use for daemon connections.
 * Priority: DAEMON_TOKEN env var (build-time) > ?token= query param > sessionStorage.
 */
export function getAuthToken(): string | null {
  if (__DAEMON_TOKEN__ !== ``)
    return __DAEMON_TOKEN__;

  const fromQs = getTokenFromQueryString();
  if (fromQs)
    return fromQs;

  try {
    return sessionStorage.getItem(SESSION_STORAGE_TOKEN_KEY);
  } catch {
    return null;
  }
}

/**
 * Persists a token in sessionStorage for the current tab.
 */
export function setAuthToken(token: string): void {
  try {
    sessionStorage.setItem(SESSION_STORAGE_TOKEN_KEY, token);
  } catch {
    // Ignore storage errors.
  }
}

function buildConnectionUrl(base: string, token: string | null): string {
  if (!token)
    return base;

  const separator = base.includes(`?`) ? `&` : `?`;
  return `${base}${separator}token=${encodeURIComponent(token)}`;
}

interface PendingRequest {
  resolve: (response: DaemonResponse) => void;
  reject: (error: Error) => void;
  timeoutId: number;
}

export class DaemonConnection {
  private url: string;
  private token: string | null;
  private socket: WebSocket | null = null;
  private nextRequestId = 1;
  private pendingRequests = new Map<number, PendingRequest>();
  private notificationListeners = new Set<NotificationListener>();
  private stateListeners = new Set<ConnectionStateListener>();
  private state: ConnectionState = `disconnected`;
  private connectionError: string | null = null;
  private reconnectTimer: number | null = null;
  private disposed = false;

  constructor(url: string, token: string | null) {
    this.url = url;
    this.token = token;
    console.log(`DaemonConnection constructor`, url, token);
    this.connect();
  }

  getState(): ConnectionState {
    return this.state;
  }

  getConnectionError(): string | null {
    return this.connectionError;
  }

  onStateChange(listener: ConnectionStateListener): () => void {
    this.stateListeners.add(listener);
    return () => this.stateListeners.delete(listener);
  }

  onNotification(listener: NotificationListener): () => void {
    this.notificationListeners.add(listener);
    return () => this.notificationListeners.delete(listener);
  }

  dispose(): void {
    this.disposed = true;

    if (this.reconnectTimer !== null) {
      window.clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }

    for (const pending of this.pendingRequests.values()) {
      window.clearTimeout(pending.timeoutId);
      pending.reject(new Error(`Connection disposed`));
    }

    this.pendingRequests.clear();
    console.trace(`Closing socket`);
    this.socket?.close();
    this.socket = null;
    this.setState(`disconnected`);
  }

  async request(request: DaemonRequest): Promise<DaemonResponse> {
    if (this.state !== `connected` || !this.socket)
      throw new Error(`Not connected to daemon`);


    const requestId = this.nextRequestId++;

    return new Promise<DaemonResponse>((resolve, reject) => {
      const timeoutId = window.setTimeout(() => {
        this.pendingRequests.delete(requestId);
        reject(new Error(`Request timed out after ${REQUEST_TIMEOUT_MS}ms`));
      }, REQUEST_TIMEOUT_MS);

      this.pendingRequests.set(requestId, {resolve, reject, timeoutId});

      const envelope: DaemonRequestEnvelope = {requestId, request};
      this.socket!.send(JSON.stringify(envelope));
    });
  }

  // Typed helpers

  async ping(): Promise<void> {
    const response = await this.request({type: `ping`});
    if (response.type === `error`)
      throw new Error(response.message);
  }

  async getMeta(): Promise<DaemonMeta> {
    const response = await this.request({type: `getMeta`});
    if (response.type === `error`)
      throw new Error(response.message);
    if (response.type !== `meta`)
      throw new Error(`Unexpected response: ${response.type}`);
    return {version: response.version, cwd: response.cwd};
  }

  async listLongLivedTasks(): Promise<Array<LongLivedTaskInfo>> {
    const response = await this.request({type: `listLongLivedTasks`});
    if (response.type === `error`)
      throw new Error(response.message);
    if (response.type !== `longLivedTaskList`)
      throw new Error(`Unexpected response: ${response.type}`);
    return response.tasks;
  }

  async getTaskOutput(taskId: string): Promise<Array<BufferedOutputLine>> {
    const response = await this.request({type: `getTaskOutput`, taskId});
    if (response.type === `error`)
      throw new Error(response.message);
    if (response.type !== `taskOutput`)
      throw new Error(`Unexpected response: ${response.type}`);
    return response.lines;
  }

  async getTaskHistory(): Promise<Array<TaskEvent>> {
    const response = await this.request({type: `getTaskHistory`});
    if (response.type === `error`)
      throw new Error(response.message);
    if (response.type !== `taskHistory`)
      throw new Error(`Unexpected response: ${response.type}`);
    return response.events;
  }

  async getStats(): Promise<Extract<DaemonResponse, {type: `stats`}>> {
    const response = await this.request({type: `getStats`});
    if (response.type === `error`)
      throw new Error(response.message);
    if (response.type !== `stats`)
      throw new Error(`Unexpected response: ${response.type}`);
    return response;
  }

  async listDeclaredTasks(): Promise<Array<DeclaredTaskInfo>> {
    const response = await this.request({type: `listDeclaredTasks`});
    if (response.type === `error`)
      throw new Error(response.message);
    if (response.type !== `declaredTaskList`)
      throw new Error(`Unexpected response: ${response.type}`);
    return response.tasks;
  }

  async shutdown(): Promise<void> {
    const response = await this.request({type: `shutdown`});
    if (response.type === `error`)
      throw new Error(response.message);
  }

  async stopTask(taskName: string, workspace: string | null): Promise<{success: boolean; error: string | null}> {
    const response = await this.request({type: `stopTask`, taskName, workspace});
    if (response.type === `error`)
      throw new Error(response.message);
    if (response.type !== `taskStopped`)
      throw new Error(`Unexpected response: ${response.type}`);
    return {success: response.success, error: response.error};
  }

  async pushTasks(
    tasks: Array<{name: string; args: string[]}>,
    workspace: string,
    contextId: string,
    opts?: {outputSubscription?: SubscriptionScope; statusSubscription?: SubscriptionScope},
  ): Promise<{taskIds: string[]; dependencyCount: number}> {
    const response = await this.request({
      type: `pushTasks`,
      tasks,
      parentTaskId: null,
      workspace,
      outputSubscription: opts?.outputSubscription ?? `none`,
      statusSubscription: opts?.statusSubscription ?? `none`,
      contextId,
    });
    if (response.type === `error`)
      throw new Error(response.message);
    if (response.type !== `tasksEnqueued`)
      throw new Error(`Unexpected response: ${response.type}`);
    return {taskIds: response.taskIds, dependencyCount: response.dependencyCount};
  }

  // Private

  private setState(newState: ConnectionState): void {
    if (this.state === newState)
      return;

    this.state = newState;
    for (const listener of this.stateListeners) {
      listener(newState);
    }
  }

  private connect(): void {
    if (this.disposed)
      return;

    this.setState(`connecting`);

    const wsUrl = buildConnectionUrl(this.url, this.token);
    const socket = new WebSocket(wsUrl);
    this.socket = socket;

    socket.addEventListener(`open`, () => {
      if (this.socket !== socket) return;
      this.setState(`connected`);
    });

    // Track whether this socket received an auth rejection so the close
    // handler knows not to reconnect.
    let rejected = false;

    socket.addEventListener(`message`, event => {
      // Don't use the `this.socket !== socket` guard here: the close event
      // may have already fired and cleared this.socket, but the message
      // still carries useful data (e.g. the auth error sent by the server
      // right before closing).

      let parsed: DaemonMessage;
      try {
        parsed = JSON.parse(String(event.data)) as DaemonMessage;
      } catch {
        return;
      }

      if (parsed.kind === `response`) {
        const pending = this.pendingRequests.get(parsed.requestId);
        if (pending) {
          this.pendingRequests.delete(parsed.requestId);
          window.clearTimeout(pending.timeoutId);
          pending.resolve(parsed.response);
        } else if (parsed.response.type === `error`) {
          // Unsolicited error (e.g. auth rejection) — surface it and stop
          // reconnecting.
          rejected = true;
          this.connectionError = parsed.response.message;
          this.disposed = true;
          this.socket = null;

          if (this.reconnectTimer !== null) {
            window.clearTimeout(this.reconnectTimer);
            this.reconnectTimer = null;
          }

          this.rejectAllPending(parsed.response.message);
          this.setState(`rejected`);
        }
      } else if (parsed.kind === `notification`) {
        for (const listener of this.notificationListeners) {
          listener(parsed.notification);
        }
      }
    });

    socket.addEventListener(`close`, () => {
      if (this.socket !== socket) return;
      this.socket = null;

      if (rejected) return;

      this.setState(`disconnected`);
      this.rejectAllPending(`Connection closed`);
      this.scheduleReconnect();
    });

    socket.addEventListener(`error`, () => {
      if (this.socket !== socket) return;
      // The close event will follow; let it handle cleanup.
    });
  }

  private rejectAllPending(reason: string): void {
    for (const pending of this.pendingRequests.values()) {
      window.clearTimeout(pending.timeoutId);
      pending.reject(new Error(reason));
    }

    this.pendingRequests.clear();
  }

  private scheduleReconnect(): void {
    if (this.disposed || this.reconnectTimer !== null)
      return;

    this.reconnectTimer = window.setTimeout(() => {
      this.reconnectTimer = null;
      this.connect();
    }, RECONNECT_DELAY_MS);
  }
}
