use std::{collections::BTreeMap, path::Path};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::{
    config::Connection,
    events::{Event, EventSink},
};

pub const ADAPTER_INTERFACE_VERSION: u32 = 1;

pub enum Command {
    Prompt(String),
    Cancel,
    Shutdown,
}

#[derive(PartialEq, Eq)]
pub enum TurnEnd {
    Complete,
    Cancelled,
    Shutdown,
}

#[async_trait]
pub trait Session: Send {
    fn owner(&self) -> &'static str;
    async fn turn(
        &mut self,
        prompt: String,
        commands: &mut mpsc::Receiver<Command>,
        events: &EventSink,
    ) -> Result<TurnEnd>;
    async fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

pub type Factory = fn(&Connection, &Path) -> Result<Box<dyn Session>>;

#[derive(Default)]
pub struct Registry {
    adapters: BTreeMap<String, Factory>,
}

impl Registry {
    pub fn register(&mut self, name: &str, version: u32, factory: Factory) -> Result<()> {
        if version != ADAPTER_INTERFACE_VERSION {
            bail!("unsupported adapter interface version");
        }
        if self.adapters.contains_key(name) {
            bail!("adapter is already registered");
        }
        self.adapters.insert(name.to_owned(), factory);
        Ok(())
    }

    pub fn open(&self, config: &Connection, workspace: &Path) -> Result<Box<dyn Session>> {
        self.adapters
            .get(&config.adapter)
            .context("unknown adapter")?(config, workspace)
    }
}

pub async fn run(
    mut session: Box<dyn Session>,
    mut commands: mpsc::Receiver<Command>,
    events: EventSink,
) -> Result<()> {
    let result = async {
        events
            .emit(Event::Ready {
                owner: session.owner(),
            })
            .await?;
        while let Some(command) = commands.recv().await {
            match command {
                Command::Prompt(prompt) => {
                    events.emit(Event::TurnStarted).await?;
                    match session.turn(prompt, &mut commands, &events).await {
                        Ok(TurnEnd::Shutdown) => break,
                        Ok(end) => {
                            events
                                .emit(Event::TurnFinished {
                                    status: if end == TurnEnd::Complete {
                                        "complete"
                                    } else {
                                        "cancelled"
                                    },
                                })
                                .await?
                        }
                        Err(error) => {
                            events
                                .emit(Event::Error {
                                    message: error.to_string(),
                                })
                                .await?;
                            events
                                .emit(Event::TurnFinished { status: "failed" })
                                .await?;
                        }
                    }
                }
                Command::Shutdown => break,
                Command::Cancel => {}
            }
        }
        Ok(())
    }
    .await;
    let close = session.close().await;
    result.and(close)
}
