mod client;
mod coordinator;
mod coordinator_commands;
mod coordinator_state;
mod events;
mod executor;
mod handlers;
mod ipc;
mod presentation;
mod scheduler;
mod server;

pub use client::{DaemonClient, PushTasksResult, StandaloneDaemonHandle};
pub use coordinator::run_daemon;
pub use coordinator_commands::{CommandSender, CoordinatorCommand};
pub use coordinator_state::SubscriptionId;
pub use events::Stream;
pub use executor::{ExecutorPool, OutputLine, TaskRunner};
pub use ipc::{
    daemon_url, AttachedLongLivedTask, BufferedOutputLine, DaemonMessage, DaemonNotification,
    DaemonRequest, DaemonRequestEnvelope, DaemonResponse, LongLivedTaskInfo, LongLivedTaskStatus,
    SubscriptionScope, TaskSubscription, DAEMON_BASE_PORT, DAEMON_SERVER_ENV, LONG_LIVED_CONTEXT_ID,
    TASK_CURRENT_ENV,
};
pub use presentation::{prefix_colors, ProgressState};
pub use scheduler::{ContextualTaskId, PreparedTask};
pub use server::bind_to_available_port;
