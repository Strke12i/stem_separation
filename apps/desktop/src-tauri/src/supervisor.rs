use analyzer_protocol::{
    Hello, Method, PROTOCOL_VERSION, Request, Response, WorkerMessage, validate_version,
};
use serde_json::Value;
use std::ffi::OsString;
use std::process::Stdio;
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

#[derive(Clone, Debug)]
pub struct WorkerLaunch {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub startup_timeout: Duration,
    pub request_timeout: Duration,
    pub shutdown_timeout: Duration,
}

impl WorkerLaunch {
    #[must_use]
    pub fn new(
        program: impl Into<OsString>,
        args: Vec<OsString>,
        startup_timeout: Duration,
    ) -> Self {
        Self {
            program: program.into(),
            args,
            startup_timeout,
            request_timeout: Duration::from_secs(5),
            shutdown_timeout: Duration::from_secs(3),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerInfo {
    pub name: String,
    pub version: String,
    pub capabilities: Vec<String>,
}

pub struct WorkerSupervisor {
    child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    info: WorkerInfo,
    launch: WorkerLaunch,
}

#[derive(Debug, Error)]
pub enum SupervisorError {
    #[error("could not start analysis worker: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("analysis worker does not expose piped stdin/stdout")]
    MissingPipe,
    #[error("analysis worker exceeded the {phase} timeout")]
    Timeout { phase: &'static str },
    #[error("analysis worker exited before sending its handshake")]
    MissingHandshake,
    #[error("analysis worker returned malformed NDJSON: {0}")]
    MalformedJson(#[source] serde_json::Error),
    #[error("analysis worker protocol error: {0}")]
    Protocol(#[from] analyzer_protocol::ProtocolError),
    #[error("unexpected message from analysis worker: {0}")]
    UnexpectedMessage(&'static str),
    #[error("analysis worker exited unexpectedly{status}")]
    WorkerExited { status: String },
    #[error("analysis worker rejected {method}: {message}")]
    WorkerRejected { method: String, message: String },
    #[error("I/O while communicating with analysis worker: {0}")]
    Io(#[from] std::io::Error),
}

impl WorkerSupervisor {
    pub async fn start(launch: WorkerLaunch) -> Result<Self, SupervisorError> {
        info!(program = ?launch.program, args = ?launch.args, "starting analysis worker");
        let mut child = Command::new(&launch.program)
            .args(&launch.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(SupervisorError::Spawn)?;

        let stdin = child.stdin.take().ok_or(SupervisorError::MissingPipe)?;
        let stdout = child.stdout.take().ok_or(SupervisorError::MissingPipe)?;
        let stderr = child.stderr.take().ok_or(SupervisorError::MissingPipe)?;
        spawn_stderr_logger(stderr);
        let mut stdout = BufReader::new(stdout).lines();

        let hello_line = match timeout(launch.startup_timeout, stdout.next_line()).await {
            Ok(Ok(Some(line))) => line,
            Ok(Ok(None)) => return Err(SupervisorError::MissingHandshake),
            Ok(Err(error)) => return Err(SupervisorError::Io(error)),
            Err(_) => return Err(SupervisorError::Timeout { phase: "startup" }),
        };
        let hello: Hello =
            serde_json::from_str(&hello_line).map_err(SupervisorError::MalformedJson)?;
        validate_version(hello.protocol_version)?;

        let info = WorkerInfo {
            name: hello.worker,
            version: hello.worker_version,
            capabilities: hello.capabilities,
        };
        info!(worker = %info.name, version = %info.version, protocol = PROTOCOL_VERSION, "analysis worker is healthy");

        Ok(Self {
            child,
            stdin,
            stdout,
            info,
            launch,
        })
    }

    #[must_use]
    pub fn info(&self) -> &WorkerInfo {
        &self.info
    }

    pub async fn ping(&mut self) -> Result<(), SupervisorError> {
        let response = self.request(Method::Ping).await?;
        if response
            .result
            .as_ref()
            .and_then(|result| result.get("pong"))
            .and_then(Value::as_bool)
            == Some(true)
        {
            Ok(())
        } else {
            Err(SupervisorError::UnexpectedMessage(
                "ping response did not include pong=true",
            ))
        }
    }

    pub async fn inspect_capabilities(&mut self) -> Result<Value, SupervisorError> {
        let response = self.request(Method::InspectCapabilities).await?;
        response.result.ok_or(SupervisorError::UnexpectedMessage(
            "capabilities response had no result",
        ))
    }

    pub async fn request(&mut self, method: Method) -> Result<Response, SupervisorError> {
        let request = Request::new(method.clone());
        let request_id = request.request_id.clone();
        let encoded = serde_json::to_string(&request).map_err(SupervisorError::MalformedJson)?;
        debug!(request_id = %request_id, method = ?method, "sending worker request");
        self.stdin.write_all(encoded.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;

        let line = match timeout(self.launch.request_timeout, self.stdout.next_line()).await {
            Ok(Ok(Some(line))) => line,
            Ok(Ok(None)) => return Err(self.unexpected_exit().await),
            Ok(Err(error)) => return Err(SupervisorError::Io(error)),
            Err(_) => return Err(SupervisorError::Timeout { phase: "request" }),
        };

        let message: WorkerMessage =
            serde_json::from_str(&line).map_err(SupervisorError::MalformedJson)?;
        match message {
            WorkerMessage::Response(response) => {
                validate_version(response.protocol_version)?;
                if response.request_id != request_id {
                    return Err(SupervisorError::UnexpectedMessage(
                        "response request_id did not match",
                    ));
                }
                if response.ok {
                    Ok(response)
                } else {
                    let message = response.error.as_ref().map_or_else(
                        || "unknown worker error".to_owned(),
                        |error| error.message.clone(),
                    );
                    Err(SupervisorError::WorkerRejected {
                        method: format!("{method:?}"),
                        message,
                    })
                }
            }
            WorkerMessage::Event(_) => Err(SupervisorError::UnexpectedMessage(
                "event received where a response was expected",
            )),
        }
    }

    pub async fn shutdown(&mut self) -> Result<(), SupervisorError> {
        match self.request(Method::Shutdown).await {
            Ok(_) => {}
            Err(error) => {
                warn!(error = %error, "graceful worker shutdown request failed; terminating worker");
                self.terminate().await?;
                return Err(error);
            }
        }

        match timeout(self.launch.shutdown_timeout, self.child.wait()).await {
            Ok(Ok(status)) if status.success() => {
                info!(status = %status, "analysis worker exited cleanly");
                Ok(())
            }
            Ok(Ok(status)) => Err(SupervisorError::WorkerExited {
                status: format!(" with exit status {status}"),
            }),
            Ok(Err(error)) => Err(SupervisorError::Io(error)),
            Err(_) => {
                warn!("analysis worker ignored graceful shutdown timeout; terminating it");
                self.terminate().await?;
                Err(SupervisorError::Timeout { phase: "shutdown" })
            }
        }
    }

    async fn unexpected_exit(&mut self) -> SupervisorError {
        match self.child.try_wait() {
            Ok(Some(status)) => {
                error!(status = %status, "analysis worker exited unexpectedly");
                SupervisorError::WorkerExited {
                    status: format!(" with exit status {status}"),
                }
            }
            Ok(None) => SupervisorError::WorkerExited {
                status: " while stdout closed".to_owned(),
            },
            Err(error) => SupervisorError::Io(error),
        }
    }

    async fn terminate(&mut self) -> Result<(), SupervisorError> {
        if self.child.try_wait()?.is_none() {
            self.child.start_kill()?;
            let _ = self.child.wait().await?;
        }
        Ok(())
    }
}

fn spawn_stderr_logger(stderr: tokio::process::ChildStderr) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            debug!(worker_stderr = %line, "analysis worker log");
        }
    });
}
