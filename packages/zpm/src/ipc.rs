use interprocess::local_socket::{
    GenericFilePath, GenericNamespaced, ListenerOptions, ToFsName, ToNsName,
    traits::tokio::{Listener, Stream},
    tokio::{prelude::*, RecvHalf, SendHalf},
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};

use crate::error::Error;

pub const IPC_SOCKET_ENV: &str = "ZPM_TASK_IPC_SOCKET";
pub const IPC_CURRENT_TASK_ENV: &str = "ZPM_TASK_CURRENT";

pub struct PushRequest {
    pub task_name: String,
    pub parent_task_id: Option<String>,
    pub response_tx: oneshot::Sender<PushResponse>,
}

pub enum PushResponse {
    Ok,
    Error(String),
}

pub struct TaskIpcServer {
    socket_name: String,
    listener: LocalSocketListener,
}

impl TaskIpcServer {
    pub async fn new() -> Result<Self, Error> {
        let pid
            = std::process::id();

        let random: u64
            = rand::random();

        let socket_name
            = format!("zpm-task-{}-{:x}.sock", pid, random);

        let name
            = socket_name.clone().to_ns_name::<GenericNamespaced>()
                .or_else(|_| socket_name.clone().to_fs_name::<GenericFilePath>())
                .map_err(|e| Error::IpcError(e.to_string()))?;

        let opts
            = ListenerOptions::new().name(name);

        let listener
            = opts.create_tokio()
                .map_err(|e| Error::IpcError(e.to_string()))?;

        Ok(Self {
            socket_name,
            listener,
        })
    }

    pub fn socket_name(&self) -> &str {
        &self.socket_name
    }

    pub async fn accept_connection(&self) -> Result<LocalSocketStream, Error> {
        self.listener.accept().await
            .map_err(|e| Error::IpcError(e.to_string()))
    }

    pub async fn run(self, task_sender: mpsc::Sender<PushRequest>) {
        loop {
            match self.accept_connection().await {
                Ok(stream) => {
                    let sender
                        = task_sender.clone();

                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(stream, sender).await {
                            eprintln!("IPC connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    eprintln!("IPC accept error: {}", e);
                }
            }
        }
    }
}

async fn handle_connection(
    stream: LocalSocketStream,
    sender: mpsc::Sender<PushRequest>,
) -> Result<(), Error> {
    let (recver, mut send)
        = stream.split();

    let mut lines
        = BufReader::new(recver).lines();

    while let Some(line) = lines.next_line().await.map_err(|e| Error::IpcError(e.to_string()))? {
        let response
            = if let Some(rest) = line.strip_prefix("PUSH ") {
                let (task_name, parent_task_id)
                    = if let Some((task, parent)) = rest.split_once(" FROM ") {
                        (task.trim().to_string(), Some(parent.trim().to_string()))
                    } else {
                        (rest.trim().to_string(), None)
                    };

                let (response_tx, response_rx)
                    = oneshot::channel();

                let request
                    = PushRequest {
                        task_name,
                        parent_task_id,
                        response_tx,
                    };

                if sender.send(request).await.is_ok() {
                    match response_rx.await {
                        Ok(PushResponse::Ok) => "OK\n".to_string(),
                        Ok(PushResponse::Error(msg)) => format!("ERR {}\n", msg),
                        Err(_) => "ERR Internal error\n".to_string(),
                    }
                } else {
                    "ERR Server shutting down\n".to_string()
                }
            } else {
                "ERR Unknown command\n".to_string()
            };

        send.write_all(response.as_bytes()).await
            .map_err(|e| Error::IpcError(e.to_string()))?;

        send.flush().await
            .map_err(|e| Error::IpcError(e.to_string()))?;
    }

    Ok(())
}

pub struct TaskIpcClient {
    send: SendHalf,
    recv: BufReader<RecvHalf>,
}

impl TaskIpcClient {
    pub async fn connect() -> Result<Self, Error> {
        let socket_name
            = std::env::var(IPC_SOCKET_ENV)
                .map_err(|_| Error::NotInTaskContext)?;

        Self::connect_to(&socket_name).await
    }

    pub async fn connect_to(socket_name: &str) -> Result<Self, Error> {
        let name
            = socket_name.to_ns_name::<GenericNamespaced>()
                .or_else(|_| socket_name.to_fs_name::<GenericFilePath>())
                .map_err(|e| Error::IpcConnectionFailed(e.to_string()))?;

        let stream
            = LocalSocketStream::connect(name).await
                .map_err(|e| Error::IpcConnectionFailed(e.to_string()))?;

        let (recv, send)
            = stream.split();

        Ok(Self {
            send,
            recv: BufReader::new(recv),
        })
    }

    pub async fn push_task(&mut self, task_name: &str, parent_task_id: Option<&str>) -> Result<(), Error> {
        let message
            = match parent_task_id {
                Some(parent) => format!("PUSH {} FROM {}\n", task_name, parent),
                None => format!("PUSH {}\n", task_name),
            };

        self.send.write_all(message.as_bytes()).await
            .map_err(|e| Error::IpcError(e.to_string()))?;

        self.send.flush().await
            .map_err(|e| Error::IpcError(e.to_string()))?;

        let mut response
            = String::new();

        self.recv.read_line(&mut response).await
            .map_err(|e| Error::IpcError(e.to_string()))?;

        if response.starts_with("OK") {
            Ok(())
        } else if let Some(msg) = response.strip_prefix("ERR ") {
            Err(Error::TaskPushFailed(msg.trim().to_string()))
        } else {
            Err(Error::IpcError(format!("Unknown response: {}", response.trim())))
        }
    }
}
