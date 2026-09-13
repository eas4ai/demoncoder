//! A public host operation with explicit ordered membership, never an inferred backend group.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchArgs {
    calls: Vec<Member>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Member {
    tool: String,
    arguments: Value,
}

pub(super) fn definition() -> Value {
    json!({"name":"tool_batch","description":"Execute 1 to 32 explicitly ordered tool calls. Each member has its own access check and result. Equal requests are separate operations; nested batches are forbidden. No transaction rollback is implied.","input_schema":{"type":"object","properties":{"calls":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"object","properties":{"tool":{"type":"string"},"arguments":{"type":"object"}},"required":["tool","arguments"],"additionalProperties":false}}},"required":["calls"],"additionalProperties":false}})
}

struct Owner {
    runtime: crate::workflow::runtime::SharedRuntime,
    id: u64,
    settled: bool,
}
impl Drop for Owner {
    fn drop(&mut self) {
        if !self.settled {
            let _ = self.runtime.interrupt_tool_batch(self.id);
        }
    }
}

impl ToolExecutor {
    pub(super) fn validate_batch(&self, value: &Value) -> Result<Vec<ToolCall>> {
        ensure!(
            serde_json::to_vec(value)?.len() <= MAX_BYTES,
            "batch input exceeds 1 MiB"
        );
        let args: BatchArgs = serde_json::from_value(value.clone())?;
        ensure!(
            (1..=32).contains(&args.calls.len()),
            "batch requires 1 to 32 members"
        );
        let definitions = self.definitions();
        args.calls
            .into_iter()
            .map(|member| {
                ensure!(
                    member.tool != "tool_batch"
                        && definitions.iter().any(|d| d["name"] == member.tool),
                    "nested or unknown batch member"
                );
                let call = ToolCall {
                    id: String::new(),
                    name: member.tool,
                    arguments: member.arguments,
                };
                self.validate_final_call(&call)?;
                Ok(call)
            })
            .collect()
    }
    pub(super) async fn execute_batch(
        &self,
        call: &ToolCall,
        events: &EventSink,
    ) -> Result<String> {
        let (events, id, members) =
            events.begin_tool_batch(self.validate_batch(&call.arguments)?)?;
        let runtime = events.batch_runtime()?;
        let mut owner = Owner {
            runtime,
            id,
            settled: false,
        };
        let mut presentations = Vec::with_capacity(members.len());
        for member in members {
            let result = Box::pin(self.execute(member, &events)).await?;
            self.validate_post_release(&result.call_id, &events).await?;
            events.complete_local_post_release(&result.call_id)?;
            presentations.push(result);
            self.take_completed();
        }
        owner.runtime.settle_tool_batch(id)?;
        owner.settled = true;
        let mut context = String::new();
        if let Some(outcome) = self
            .dispatch_non_tool(owner.runtime.batch_occurrence(id)?, &events)
            .await?
        {
            ensure!(
                outcome.hold.is_none() && !outcome.correction,
                "batch lifecycle continuation unmet: {}",
                outcome.hold.as_deref().unwrap_or("correction required")
            );
            // Context belongs to this wrapper's result, never an invented new developer submission.
            context = outcome.context;
        }
        let mut output = serde_json::to_string(&presentations)?;
        if !context.is_empty() {
            output.push('\n');
            output.push_str(&context);
        }
        ensure!(
            output.len() <= MAX_BYTES,
            "batch result exceeds 1 MiB; individual evidence remains retained"
        );
        Ok(output)
    }
}
