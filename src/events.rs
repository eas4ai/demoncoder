use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use serde::Serialize;
use tokio::sync::mpsc;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Ready {
        owner: &'static str,
    },
    TurnStarted,
    Text {
        text: String,
    },
    ToolStarted {
        call: crate::tools::ToolCall,
    },
    ToolOutput {
        call_id: String,
        stream: &'static str,
        text: String,
    },
    ToolFinished {
        result: crate::tools::ToolResult,
    },
    ToolPresentation {
        call_id: String,
        text: String,
    },
    Usage {
        input: Option<u64>,
        output: Option<u64>,
        cached: Option<u64>,
        cost_usd: Option<f64>,
    },
    TurnFinished {
        status: &'static str,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct Envelope {
    pub connection: String,
    pub event: Event,
}

/// One ordered publication path for both retained events and the UI.
#[derive(Clone)]
pub struct EventSink {
    connection: String,
    sender: mpsc::Sender<Envelope>,
    log: Option<Arc<Mutex<File>>>,
}

impl EventSink {
    pub fn new(
        connection: String,
        sender: mpsc::Sender<Envelope>,
        path: Option<&Path>,
    ) -> Result<Self> {
        let log = path
            .map(|path| {
                let mut options = OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                options
                    .open(path)
                    .context("create a new event log")
                    .map(|file| Arc::new(Mutex::new(file)))
            })
            .transpose()?;
        Ok(Self {
            connection,
            sender,
            log,
        })
    }

    pub async fn emit(&self, event: Event) -> Result<()> {
        let envelope = Envelope {
            connection: self.connection.clone(),
            event,
        };
        if let Some(log) = &self.log {
            let mut log = log
                .lock()
                .map_err(|_| anyhow::anyhow!("event log lock failed"))?;
            serde_json::to_writer(&mut *log, &envelope).context("serialize session event")?;
            log.write_all(b"\n").context("append session event")?;
            log.flush().context("flush session event")?;
        }
        self.sender
            .send(envelope)
            .await
            .context("terminal event receiver closed")
    }
}
