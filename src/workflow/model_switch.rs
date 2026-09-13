//! Host-owned dispatch for Creator identity transitions.
use crate::{
    config::Connection,
    events::EventSink,
    plugins::{
        gate_snapshot::GateWorkspace,
        hook_types::HookEvent,
        non_tool::{NonToolOutcome, NonToolPlan},
        receipts::NonToolOccurrence,
        runners::HookHost,
    },
};
use anyhow::{Context, Result};
use std::{fs::File, os::unix::fs::MetadataExt, path::Path, sync::Arc};

pub(super) struct Owner {
    runtime: super::runtime::SharedRuntime,
    id: u64,
    finished: bool,
}

impl Owner {
    pub(super) fn new(runtime: super::runtime::SharedRuntime, id: u64) -> Self {
        Self {
            runtime,
            id,
            finished: false,
        }
    }

    pub(super) fn finish(&mut self, hold: Option<String>) -> Result<()> {
        self.runtime.end_model_switch(self.id, hold)?;
        self.finished = true;
        Ok(())
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.runtime.interrupt_model_switch(self.id);
        }
    }
}

pub(super) struct Plans {
    pre: Option<Arc<NonToolPlan>>,
    post: Option<Arc<NonToolPlan>>,
    workspace: Arc<GateWorkspace>,
    workspace_identity: (u64, u64),
    host: HookHost,
}

impl Plans {
    pub(super) fn new(workspace: &Path, connection: &Connection) -> Result<Self> {
        let canonical = workspace
            .canonicalize()
            .context("resolve model switch workspace")?;
        let gate = Arc::new(GateWorkspace::open_with_credentials(
            &canonical,
            &connection.access.credential_paths,
        )?);
        let root = Arc::new(File::open(&canonical).context("open model switch workspace")?);
        let metadata = root.metadata()?;
        let plan = |event| {
            connection
                .access
                .non_tools
                .iter()
                .find(|plan| plan.plan.event == event)
                .cloned()
        };
        Ok(Self {
            pre: plan(HookEvent::PreModelSwitch),
            post: plan(HookEvent::PostModelSwitch),
            workspace: gate.clone(),
            workspace_identity: (metadata.dev(), metadata.ino()),
            host: HookHost::new(
                root,
                canonical,
                gate.frozen_credentials(),
                connection.access.supervisor.clone(),
            ),
        })
    }

    pub(super) fn pre_digest(&self) -> Option<String> {
        self.pre.as_ref().map(|plan| plan.plan.digest.clone())
    }

    pub(super) fn post_digest(&self) -> Option<String> {
        self.post.as_ref().map(|plan| plan.plan.digest.clone())
    }

    pub(super) fn source_pre_digest(&self) -> Result<String> {
        self.pre_digest().map_or_else(
            || NonToolPlan::authenticated_source_observation_digest(HookEvent::PreModelSwitch),
            Ok,
        )
    }

    pub(super) fn source_post_digest(&self) -> Result<String> {
        self.post_digest().map_or_else(
            || NonToolPlan::authenticated_source_observation_digest(HookEvent::PostModelSwitch),
            Ok,
        )
    }

    pub(super) async fn pre(
        &self,
        events: &EventSink,
        switch: u64,
        requested_model: Option<String>,
        source: &str,
    ) -> Result<Option<NonToolOutcome>> {
        let Some(plan) = &self.pre else {
            return Ok(None);
        };
        plan.dispatch(
            NonToolOccurrence::PreModelSwitch {
                model_switch: switch,
                resolved_model: requested_model.clone(),
                requested_model,
                source: source.into(),
            },
            events,
            self.workspace.clone(),
            self.workspace_identity,
            self.host.clone(),
        )
        .await
        .map(Some)
    }

    pub(super) async fn post(
        &self,
        events: &EventSink,
        switch: u64,
        model: Option<String>,
        source: &str,
    ) -> Result<Option<NonToolOutcome>> {
        let Some(plan) = &self.post else {
            return Ok(None);
        };
        plan.dispatch(
            NonToolOccurrence::PostModelSwitch {
                model_switch: switch,
                model,
                source: source.into(),
            },
            events,
            self.workspace.clone(),
            self.workspace_identity,
            self.host.clone(),
        )
        .await
        .map(Some)
    }
}
