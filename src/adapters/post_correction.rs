//! External corrections supersede a backend turn before any new model work.
//! The retained host result is evidence; a cancelled backend placeholder is not.
use crate::{events::EventSink, tools::ToolExecutor, workflow::runtime::SharedRuntime};
use anyhow::{Context, Result, ensure};

pub(super) struct ExternalCorrection {
    events: EventSink,
    runtime: SharedRuntime,
    operation: u64,
    call_id: String,
    pub(super) prompt: String,
    pub(super) invocation: Option<u64>,
    claude_blocks: Option<Vec<serde_json::Value>>,
    pub(super) request: Option<serde_json::Value>,
}

impl ExternalCorrection {
    pub(super) fn start(events: &EventSink, call_id: &str) -> Result<Self> {
        let (runtime, operation) = events
            .post_delivery_context(call_id)?
            .context("external correction has no retained post-tool owner")?;
        let record = runtime.record()?;
        let tool = record
            .operations
            .iter()
            .find(|o| o.id == operation)
            .context("external correction evidence missing")?;
        let original = tool
            .result
            .as_ref()
            .context("original host result missing")?;
        let post = tool
            .tool_receipt
            .as_ref()
            .and_then(|r| r.plugin_lifecycle.as_ref())
            .context("external correction lifecycle missing")?;
        let objective = runtime.post_correction_objective(operation)?;
        let context = post
            .messages
            .iter()
            .map(|m| format!("[Plugin-origin {}] {}", m.package, m.text))
            .collect::<Vec<_>>()
            .join("\n");
        let claude_blocks = post
            .facts
            .representation
            .is_mcp()
            .then(|| {
                post.model_content
                    .as_ref()
                    .and_then(serde_json::Value::as_array)
                    .cloned()
            })
            .flatten();
        let presentation = if claude_blocks.is_some() {
            "Exact structured provider content follows this attributed context.".to_owned()
        } else {
            serde_json::to_string(
                &post
                    .model_content
                    .as_ref()
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!(tool.model_result())),
            )?
        };
        let prompt = format!(
            "Continue the original owning task with this bounded plugin correction.\n\
            The previous backend turn was interrupted after the host tool completed. Any backend message claiming the tool was rejected or not executed is interruption bookkeeping, not a developer denial and not evidence that the completed host operation failed. Do not repeat its completed effect merely because the backend recorded that placeholder.\n\
            Original task: {objective}\n\
            [Immutable host execution evidence]\n{}\n\
            [Plugin presentation; separate from host execution evidence]\n{}\n{context}",
            serde_json::to_string(original)?,
            presentation,
        );
        ensure!(
            prompt.len() <= 4 * 1024 * 1024,
            "external correction context exceeds bound"
        );
        runtime.start_post_supersession(operation)?;
        Ok(Self {
            events: events.clone(),
            runtime,
            operation,
            call_id: call_id.into(),
            prompt,
            invocation: None,
            claude_blocks,
            request: None,
        })
    }

    /// Prepare the source user content before charging/reserving its invocation.
    /// Evidence and attribution stay text; provider blocks are never stringified
    /// or wrapped in a fabricated tool_result using the superseded tool ID.
    pub(super) fn prepare_claude_user(
        &mut self,
        context: &str,
        session: &str,
        uuid: &str,
    ) -> Result<()> {
        let content = if let Some(blocks) = &self.claude_blocks {
            let mut content = vec![serde_json::json!({"type":"text","text":context})];
            content.extend(blocks.iter().cloned());
            serde_json::Value::Array(content)
        } else {
            serde_json::Value::String(context.into())
        };
        self.request = Some(prepare_claude_frame(content, session, uuid)?);
        Ok(())
    }

    pub(super) fn prepare_codex_request(
        &mut self,
        context: &str,
        thread: &str,
        effort: Option<&str>,
        id: u64,
    ) -> Result<()> {
        self.request = Some(prepare_codex_frame(context, thread, effort, id)?);
        Ok(())
    }

    /// Call only after both the exact interrupt acknowledgment and the original
    /// backend's terminal interruption have been observed. No ordinary response
    /// is delivered for the superseded tool/callback.
    pub(super) async fn reserve(&mut self, tools: &ToolExecutor) -> Result<u64> {
        ensure!(
            self.request.is_some(),
            "external correction frame must be prepared before reservation"
        );
        self.runtime.finish_post_supersession(self.operation)?;
        tools
            .validate_post_release(&self.call_id, &self.events)
            .await?;
        // The external owner polls its command queue before polling this future
        // again, including a Cancel queued by the final validation's same poll.
        tokio::task::yield_now().await;
        let invocation = self.events.reserve_post_correction(&self.call_id)?;
        self.invocation = Some(invocation);
        Ok(invocation)
    }

    pub(super) fn source_origin(
        &self,
        events: &EventSink,
        user: &serde_json::Value,
    ) -> Result<crate::plugins::receipts::SourceOrigin> {
        ensure!(
            self.invocation.is_some()
                && self.invocation == events.backend_invocation_id()
                && self.request.as_ref() == Some(user),
            "source Submit differs from the exact reserved correction frame"
        );
        Ok(
            crate::plugins::receipts::SourceOrigin::PluginPostCorrection {
                post_operation: self.operation,
                content_digest: crate::plugins::admission::digest(
                    if user["method"] == "turn/start" {
                        &user["params"]["input"]
                    } else {
                        &user["message"]
                    },
                )?,
            },
        )
    }

    pub(super) fn acknowledge(
        &self,
        acknowledgment: crate::plugins::receipts::CorrectionAcknowledgment,
    ) -> Result<()> {
        self.runtime.ack_post_correction(
            self.operation,
            self.invocation.context("correction was not reserved")?,
            acknowledgment,
        )
    }
}

/// Budget the actual user frame and the pinned SDK replay envelope before
/// spending the correction round. JSON escaping and the terminating newline
/// count toward the same limit used by BackendProcess::receive.
fn prepare_claude_frame(
    content: serde_json::Value,
    session: &str,
    uuid: &str,
) -> Result<serde_json::Value> {
    let user = serde_json::json!({"type":"user","message":{"role":"user","content":content},"parent_tool_use_id":null,"session_id":session,"uuid":uuid});
    let mut replay = user.clone();
    replay["isReplay"] = serde_json::json!(true);
    // Pinned Claude copies Date.toISOString(): its signed six-digit year
    // form is 27 ASCII bytes, longer than ordinary four-digit-year dates.
    replay["timestamp"] = serde_json::json!("+010000-01-01T00:00:00.000Z");
    for frame in [&user, &replay] {
        validate_frame(frame, "Claude")?;
    }
    Ok(user)
}

/// The pinned Codex source emits the submitted text in both userMessage
/// notifications, independently of its turn/start RPC acknowledgment. These
/// fresh source-generated item/turn IDs are UUIDs; timestamps are nonnegative
/// i64 milliseconds. Budget their maximum serialized forms and the actual
/// retained thread ID, without assuming a fixed thread-ID size.
fn prepare_codex_frame(
    context: &str,
    thread: &str,
    effort: Option<&str>,
    id: u64,
) -> Result<serde_json::Value> {
    let request = serde_json::json!({"id":id,"method":"turn/start","params":{
        "threadId":thread,"input":[{"type":"text","text":context}],
        "environments":[],"effort":effort,
    }});
    validate_frame(&request, "Codex")?;
    const UUID: &str = "00000000-0000-4000-8000-000000000000";
    for (method, clock) in [
        ("item/started", "startedAtMs"),
        ("item/completed", "completedAtMs"),
    ] {
        let mut notification = serde_json::json!({"method":method,"params":{
            "item":{"type":"userMessage","id":UUID,"clientId":null,
                "content":[{"type":"text","text":context,"text_elements":[]}]},
            "threadId":thread,"turnId":UUID,
        },"emittedAtMs":i64::MAX});
        notification["params"][clock] = serde_json::json!(i64::MAX);
        validate_frame(&notification, "Codex")?;
    }
    Ok(request)
}

fn validate_frame(frame: &serde_json::Value, backend: &str) -> Result<()> {
    ensure!(
        serde_json::to_vec(frame)?.len() < super::process::RESPONSE_FRAME_LIMIT,
        "{backend} correction frame exceeds transport bound"
    );
    Ok(())
}

/// One transport clock owns the pending correction handshake. Every adapter
/// checks it before dispatch as well as while awaiting I/O, so ready traffic
/// cannot extend the handshake. The outer session owns cancellation priority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransportStage {
    Acknowledgment,
    Supersession,
}

#[derive(Clone)]
pub(super) struct CorrectionDeadline {
    pending: tokio::sync::watch::Sender<Option<(TransportStage, tokio::time::Instant)>>,
}

impl CorrectionDeadline {
    const LIMIT: std::time::Duration = std::time::Duration::from_secs(30);

    pub(super) fn new(awaiting_acknowledgment: bool) -> Self {
        let pending = awaiting_acknowledgment.then(|| {
            (
                TransportStage::Acknowledgment,
                tokio::time::Instant::now() + Self::LIMIT,
            )
        });
        Self {
            pending: tokio::sync::watch::channel(pending).0,
        }
    }

    pub(super) fn supersede(&mut self) {
        let next = tokio::time::Instant::now() + Self::LIMIT;
        self.pending.send_modify(|pending| {
            // Steering cannot extend an already pending acknowledgment.
            let at = pending.map_or(next, |(_, previous)| previous.min(next));
            *pending = Some((TransportStage::Supersession, at));
        });
    }

    pub(super) fn acknowledged(&mut self) -> Result<()> {
        let mut expired = None;
        self.pending.send_if_modified(|pending| {
            if let Some((stage, at)) = *pending
                && tokio::time::Instant::now() >= at
            {
                expired = Some(stage);
                return false;
            }
            if matches!(pending, Some((TransportStage::Acknowledgment, _))) {
                *pending = None;
                true
            } else {
                false
            }
        });
        match expired {
            Some(stage) => Err(Self::timeout_error(stage)),
            None => Ok(()),
        }
    }

    fn timeout_error(stage: TransportStage) -> anyhow::Error {
        let name = match stage {
            TransportStage::Acknowledgment => "acknowledgment",
            TransportStage::Supersession => "supersession",
        };
        anyhow::anyhow!("Backend correction {name} timed out after 30 seconds")
    }

    /// Includes partial writes in the same handshake budget. A timed-out write
    /// remains uncertain; its caller closes the transport without retrying it.
    pub(super) async fn during<T>(
        &self,
        operation: impl std::future::Future<Output = Result<T>>,
    ) -> Result<T> {
        self.check()?;
        tokio::pin!(operation);
        let result = loop {
            tokio::select! {
                biased;
                expired = self.wait() => { expired?; },
                result = &mut operation => break result,
            }
        };
        self.check()?;
        result.map_err(|error| match *self.pending.borrow() {
            Some((stage, _)) => {
                let context = format!(
                    "Backend correction {} failed: {error}",
                    match stage {
                        TransportStage::Acknowledgment => "acknowledgment",
                        TransportStage::Supersession => "supersession",
                    }
                );
                error.context(context)
            }
            None => error,
        })
    }

    pub(super) fn check(&self) -> Result<()> {
        if let Some((stage, at)) = *self.pending.borrow()
            && tokio::time::Instant::now() >= at
        {
            return Err(Self::timeout_error(stage));
        }
        Ok(())
    }

    /// All dispatch handles observe the owner's current stage. A timely exact
    /// acknowledgment removes its timer even if that dispatch keeps working;
    /// a stage activated during an await uses its existing absolute deadline.
    pub(super) async fn wait(&self) -> Result<()> {
        let mut pending = self.pending.subscribe();
        loop {
            self.check()?;
            let current = *pending.borrow_and_update();
            if let Some((_, at)) = current {
                tokio::select! {
                    biased;
                    changed = pending.changed() => { changed.context("correction clock closed")?; },
                    _ = tokio::time::sleep_until(at) => {},
                }
            } else {
                pending.changed().await.context("correction clock closed")?;
            }
        }
    }
}

#[cfg(test)]
mod timing_tests {
    use super::*;

    #[test]
    fn source_submit_proof_requires_the_live_exact_frozen_frame_and_backend() {
        let root = tempfile::tempdir().unwrap();
        let runtime = SharedRuntime::for_test(
            &root.path().join("record"),
            crate::inspection::tests::record(root.path()),
        )
        .unwrap();
        let (sender, _receiver) = tokio::sync::mpsc::channel(8);
        let events = EventSink::new("test".into(), sender, None)
            .unwrap()
            .for_invocation(Some(7));
        let user = prepare_claude_frame(serde_json::json!([{"type":"text","text":"plugin feedback"},{"type":"image","source":{"type":"base64","data":"retained"}}]), "source", "00000000-0000-4000-8000-000000000001").unwrap();
        let mut handoff = ExternalCorrection {
            events: events.clone(),
            runtime,
            operation: 3,
            call_id: "completed".into(),
            prompt: "plugin feedback".into(),
            invocation: Some(7),
            claude_blocks: None,
            request: Some(user.clone()),
        };
        assert!(matches!(
            handoff.source_origin(&events, &user).unwrap(),
            crate::plugins::receipts::SourceOrigin::PluginPostCorrection {
                post_operation: 3,
                ..
            }
        ));
        for field in ["uuid", "session_id", "message"] {
            let mut changed = user.clone();
            changed[field] = serde_json::json!("forged");
            assert!(handoff.source_origin(&events, &changed).is_err(), "{field}");
        }
        assert!(
            handoff
                .source_origin(&events.for_invocation(Some(8)), &user)
                .is_err()
        );
        handoff.invocation = None;
        assert!(handoff.source_origin(&events, &user).is_err());
    }

    #[test]
    fn complete_claude_replay_frame_counts_escaping_multibyte_and_newline() {
        let uuid = "00000000-0000-4000-8000-000000000000";
        for typed in [false, true] {
            let content = |text: String| {
                if typed {
                    serde_json::json!([{"type":"text","text":text},{"type":"image","source":{"type":"url","url":"https://example.test/é.png"}}])
                } else {
                    serde_json::json!(text)
                }
            };
            let mut replay = prepare_claude_frame(content(String::new()), "session", uuid).unwrap();
            replay["isReplay"] = serde_json::json!(true);
            replay["timestamp"] = serde_json::json!("+010000-01-01T00:00:00.000Z");
            let available = super::super::process::RESPONSE_FRAME_LIMIT
                - serde_json::to_vec(&replay).unwrap().len()
                - 1;
            for token in ["x", "é", "\\"] {
                let encoded = serde_json::to_vec(token).unwrap().len() - 2;
                let text = format!(
                    "{}{}",
                    token.repeat(available / encoded),
                    "x".repeat(available % encoded)
                );
                assert!(prepare_claude_frame(content(text.clone()), "session", uuid).is_ok());
                assert!(
                    prepare_claude_frame(content(format!("{text}x")), "session", uuid).is_err()
                );
            }
        }
    }

    #[test]
    fn complete_codex_user_notification_counts_source_envelope_and_request_fields() {
        const UUID: &str = "00000000-0000-4000-8000-000000000000";
        // The pinned source's empty completed userMessage with UUID thread,
        // maximum timestamps and newline occupies 354 bytes (started: 350).
        let available = super::super::process::RESPONSE_FRAME_LIMIT - 354;
        for token in ["x", "é", "\\"] {
            let encoded = serde_json::to_vec(token).unwrap().len() - 2;
            let text = format!(
                "{}{}",
                token.repeat(available / encoded),
                "x".repeat(available % encoded)
            );
            assert!(prepare_codex_frame(&text, UUID, None, u64::MAX).is_ok());
            assert!(prepare_codex_frame(&format!("{text}x"), UUID, None, u64::MAX).is_err());
            // The actual thread field is serialized, including escaping.
            assert!(prepare_codex_frame(&text, &format!("{UUID}é"), None, u64::MAX).is_err());
        }
        assert!(
            prepare_codex_frame(
                "small",
                UUID,
                Some(&"\\".repeat(super::super::process::RESPONSE_FRAME_LIMIT / 2)),
                u64::MAX
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn expired_transport_stage_rejects_ready_dispatch_and_ack_only_clears_its_stage() {
        for stage in [TransportStage::Acknowledgment, TransportStage::Supersession] {
            let mut deadline = CorrectionDeadline {
                pending: tokio::sync::watch::channel(Some((
                    stage,
                    tokio::time::Instant::now() - std::time::Duration::from_secs(1),
                )))
                .0,
            };
            assert!(deadline.check().is_err());
            let mut dispatched = false;
            assert!(
                deadline
                    .during(async {
                        dispatched = true;
                        Ok(())
                    })
                    .await
                    .is_err()
            );
            assert!(
                !dispatched,
                "expired handshake must not start another write"
            );
            assert!(deadline.wait().await.is_err());
            assert!(deadline.acknowledged().is_err());
            assert!(
                deadline.check().is_err(),
                "late acknowledgment cannot erase expiry"
            );
        }
    }

    fn short_deadline(stage: TransportStage) -> CorrectionDeadline {
        CorrectionDeadline {
            pending: tokio::sync::watch::channel(Some((
                stage,
                tokio::time::Instant::now() + std::time::Duration::from_millis(200),
            )))
            .0,
        }
    }

    #[tokio::test]
    async fn dispatch_handles_follow_live_acknowledgment_and_stage_activation() {
        let mut owner = short_deadline(TransportStage::Acknowledgment);
        owner
            .clone()
            .during(async {
                owner.acknowledged()?;
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                Ok(())
            })
            .await
            .unwrap();
        assert!(owner.check().is_ok());

        let mut owner = CorrectionDeadline::new(false);
        let result = owner
            .clone()
            .during(async {
                owner.supersede();
                owner.pending.send_replace(
                    *short_deadline(TransportStage::Supersession)
                        .pending
                        .borrow(),
                );
                owner.acknowledged()?; // a user acknowledgment cannot clear supersession
                std::future::pending::<Result<()>>().await
            })
            .await;
        assert!(result.unwrap_err().to_string().contains("supersession"));

        let mut owner = short_deadline(TransportStage::Acknowledgment);
        let error: Result<()> = owner
            .clone()
            .during(async {
                owner.acknowledged()?;
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                anyhow::bail!("ordinary dispatch failure")
            })
            .await;
        assert_eq!(error.unwrap_err().to_string(), "ordinary dispatch failure");
        let error: Result<()> = CorrectionDeadline::new(true)
            .during(async { anyhow::bail!("callback denied") })
            .await;
        let error = error.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("acknowledgment failed: callback denied")
        );
        assert_eq!(error.root_cause().to_string(), "callback denied");
    }

    #[tokio::test]
    async fn expired_dispatch_does_not_poll_io_callbacks_or_reservation_and_drops_held_locks() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let expired = short_deadline(TransportStage::Supersession);
        expired
            .pending
            .send_modify(|pending| pending.as_mut().unwrap().1 = tokio::time::Instant::now());
        let (mut writer, mut reader) = tokio::io::duplex(8);
        assert!(
            expired
                .during(async {
                    writer.write_all(b"reply").await?;
                    Ok(())
                })
                .await
                .is_err()
        );
        drop(writer);
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.unwrap();
        assert!(
            bytes.is_empty(),
            "expired dispatch must not write a response"
        );
        let lock = tokio::sync::Mutex::new(());
        let mut entered = false;
        assert!(
            expired
                .during(async {
                    let _guard = lock.lock().await;
                    entered = true;
                    Ok(())
                })
                .await
                .is_err()
        );
        assert!(
            !entered,
            "expired dispatch must not enter a callback or reserve work"
        );

        let deadline = short_deadline(TransportStage::Supersession);
        assert!(
            deadline
                .during(async {
                    let _guard = lock.lock().await;
                    std::future::pending::<Result<()>>().await
                })
                .await
                .is_err()
        );
        assert!(
            lock.try_lock().is_ok(),
            "expiration drops pending work and its lock"
        );

        let deadline = CorrectionDeadline::new(true);
        let (cancel, cancelled) = tokio::sync::oneshot::channel();
        tokio::select! {
            biased;
            _ = cancelled => {},
            result = deadline.during(async {
                let _guard = lock.lock().await;
                cancel.send(()).unwrap();
                std::future::pending::<Result<()>>().await
            }) => panic!("cancellation must own dispatch: {result:?}"),
        }
        assert!(
            lock.try_lock().is_ok(),
            "cancellation drops pending work and its lock"
        );
    }

    #[test]
    fn steering_does_not_extend_pending_transport_deadline() {
        let mut deadline = CorrectionDeadline::new(true);
        let before = deadline.pending.borrow().unwrap().1;
        deadline.supersede();
        assert_eq!(
            *deadline.pending.borrow(),
            Some((TransportStage::Supersession, before))
        );
    }
}
