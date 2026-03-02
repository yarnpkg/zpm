mod client;
mod coordinator;
mod events;
mod executor;
mod handlers;
mod ipc;
mod presentation;
mod scheduler;
mod server;
mod subscriptions;

pub use client::{DaemonClient, PushTasksResult, StandaloneDaemonHandle};
pub use coordinator::run_daemon;
pub use events::{ExecutorEvent, SchedulerEvent, Stream};
pub use executor::{ExecutorPool, OutputLine, TaskRunner};
pub use handlers::dispatch_request;
pub use ipc::{
    daemon_url, BufferedOutputLine, DaemonMessage, DaemonNotification, DaemonRequest,
    DaemonRequestEnvelope, DaemonResponse, SubscriptionScope, TaskSubscription,
    DAEMON_BASE_PORT, DAEMON_SERVER_ENV, TASK_CURRENT_ENV,
};
pub use presentation::{prefix_colors, ProgressState};
pub use scheduler::{format_task_id, PreparedTask, Scheduler};
pub use server::{bind_to_available_port, run_accept_loop, ConnectionContext};
pub use subscriptions::{SubscriptionGuard, SubscriptionId, SubscriptionRegistry};
