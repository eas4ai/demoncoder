//! Decode untrusted outcomes into retained holds and proposed rewrites only.
use crate::plugins::{
    profile::CompatibilityProfile,
    receipts::*,
    results::{DecisionChoice, GateDisposition, ProposedEffect, SourceDecision},
};
use serde_json::Value;
pub(super) fn decode(
    profile: &CompatibilityProfile,
    receipt: &mut HookReceipt,
    rewrite: &mut Option<Value>,
) {
    let raw = receipt.outcome.as_ref().expect("runner settled");
    let bounded = raw.within_retention_bound();
    if !bounded {
        receipt.outcome = Some(RawOutcome::Failure {
            reason: "handler output exceeded retention bound".into(),
        });
        receipt.hold = Some("handler output exceeded retention bound".into());
    }
    receipt.uncertain_effects = matches!(
        receipt.outcome,
        Some(RawOutcome::Failure { .. } | RawOutcome::CommandFailure { .. })
    );
    let decoded = receipt
        .outcome
        .as_ref()
        .expect("retained")
        .decode(profile, &receipt.declaration);
    if decoded.failed() || decoded.gate == GateDisposition::Held {
        receipt
            .hold
            .get_or_insert("handler returned a blocking or invalid result".into());
    }
    if receipt.class != HandlerClass::Transformer
        && decoded.source_decision == SourceDecision::NoSourceDecision
    {
        receipt
            .hold
            .get_or_insert("source result has no required decision semantics".into());
    }
    for (proposal_index, proposal) in decoded.effects.into_iter().enumerate() {
        apply_proposal(receipt, proposal_index, proposal, rewrite);
    }
}
fn apply_proposal(
    receipt: &mut HookReceipt,
    proposal_index: usize,
    proposal: ProposedEffect,
    rewrite: &mut Option<Value>,
) {
    let pending = PendingProposal {
        index: proposal_index,
        kind: (&proposal).into(),
    };
    match proposal {
        ProposedEffect::Decision {
            choice: DecisionChoice::NoObjection,
            ..
        } => {}
        ProposedEffect::Decision { choice, reason } => {
            let choice = match choice {
                DecisionChoice::Deny => PendingDecision::Deny,
                DecisionChoice::Ask => PendingDecision::Ask,
                _ => PendingDecision::Defer,
            };
            receipt.questions.push(Question {
                choice,
                reason: reason.map(|r| r.get().clone()),
            });
            receipt
                .hold
                .get_or_insert("plugin decision holds the final candidate".into());
        }
        ProposedEffect::RewriteInput(value) => {
            if receipt.class == HandlerClass::DecisionGate {
                receipt
                    .hold
                    .get_or_insert("final decision gate cannot rewrite frozen input".into());
            } else if let Some(existing) = rewrite.as_ref() {
                if existing != value.get() {
                    receipt
                        .hold
                        .get_or_insert("concurrent rewrite conflict".into());
                }
            } else {
                *rewrite = Some(value.get().clone());
            }
        }
        ProposedEffect::AdditionalContext(_)
        | ProposedEffect::Warning(_)
        | ProposedEffect::TransientNotice(_)
        | ProposedEffect::TerminalNotification(_) => {
            receipt.pending_proposals.push(pending);
        }
        _ => {
            receipt.pending_proposals.push(pending);
            receipt
                .hold
                .get_or_insert("required proposal owner is not integrated".into());
        }
    }
}
