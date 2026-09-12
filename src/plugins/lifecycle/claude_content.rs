//! Source-supported provider tool-result content, preserved as its original Value.
//! Shapes follow the standard Anthropic SDK 0.93.0 union and pinned Claude
//! transport qualification. This validates syntax and media signatures, not full
//! media decoding, provider access, or permission to fetch URLs with host tools.
use anyhow::{Context, Result, ensure};
use base64::Engine;
use serde_json::{Map, Value};

pub(crate) fn validate(value: &Value, previous_search: Option<bool>) -> Result<()> {
    super::super::wire::measure(value)?;
    if value.is_string() {
        return Ok(());
    }
    let blocks = value
        .as_array()
        .context("provider content must be a string or block array")?;
    for block in blocks {
        validate_block(block)?;
    }
    if let Some(policy) = search_policy(value)? {
        ensure!(
            previous_search.is_none_or(|previous| previous == policy),
            "Claude request compatibility: search citation setting differs from retained source conversation"
        );
    }
    Ok(())
}

fn object<'a>(value: &'a Value, allowed: &[&str]) -> Result<&'a Map<String, Value>> {
    let object = value
        .as_object()
        .context("provider block or nested field must be an object")?;
    ensure!(
        object.keys().all(|key| allowed.contains(&key.as_str())),
        "unsupported provider field"
    );
    Ok(object)
}

fn text<'a>(object: &'a Map<String, Value>, field: &str) -> Result<&'a str> {
    object
        .get(field)
        .and_then(Value::as_str)
        .context("required provider string field missing or malformed")
}

fn nullable_text(object: &Map<String, Value>, field: &str, required: bool) -> Result<()> {
    ensure!(
        object
            .get(field)
            .map_or(!required, |value| value.is_null() || value.is_string()),
        "provider nullable text field malformed"
    );
    Ok(())
}

fn cache(object: &Map<String, Value>) -> Result<()> {
    if let Some(value) = object.get("cache_control").filter(|value| !value.is_null()) {
        let cache = self::object(value, &["type", "ttl"])?;
        ensure!(
            text(cache, "type")? == "ephemeral",
            "unsupported cache control type"
        );
        ensure!(
            cache
                .get("ttl")
                .is_none_or(|ttl| matches!(ttl.as_str(), Some("5m" | "1h"))),
            "unsupported cache lifetime"
        );
    }
    Ok(())
}

fn citation_setting(object: &Map<String, Value>, nullable: bool) -> Result<bool> {
    let Some(value) = object.get("citations") else {
        return Ok(false);
    };
    if value.is_null() && nullable {
        return Ok(false);
    }
    let config = self::object(value, &["enabled"])?;
    ensure!(
        config.get("enabled").is_none_or(Value::is_boolean),
        "citation enabled must be boolean"
    );
    Ok(config
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false))
}

fn validate_block(value: &Value) -> Result<()> {
    match value["type"].as_str() {
        Some("text") => validate_text(value),
        Some("image") => validate_image(value),
        Some("document") => validate_document(value),
        Some("search_result") => validate_search(value),
        Some("tool_reference") => {
            let block = object(value, &["type", "tool_name", "cache_control"])?;
            ensure!(
                !text(block, "tool_name")?.is_empty(),
                "tool reference name must not be empty"
            );
            cache(block)?;
            anyhow::bail!(
                "Claude source capability unavailable: tool search is disabled; tool references cannot be preserved"
            )
        }
        _ => anyhow::bail!("unsupported provider content block type"),
    }
}

fn validate_text(value: &Value) -> Result<()> {
    let block = object(value, &["type", "text", "cache_control", "citations"])?;
    text(block, "text")?;
    cache(block)?;
    if let Some(value) = block.get("citations").filter(|value| !value.is_null()) {
        for citation in value
            .as_array()
            .context("text citations must be an array")?
        {
            validate_citation(citation)?;
        }
    }
    Ok(())
}

fn validate_image(value: &Value) -> Result<()> {
    let block = object(value, &["type", "source", "cache_control"])?;
    cache(block)?;
    let source = &value["source"];
    match source["type"].as_str() {
        Some("base64") => validate_media(source, false),
        Some("url") => validate_url_source(source),
        Some("file") => unavailable_file(source),
        _ => anyhow::bail!("unsupported provider image source"),
    }
}

fn validate_document(value: &Value) -> Result<()> {
    let block = object(
        value,
        &[
            "type",
            "source",
            "cache_control",
            "citations",
            "context",
            "title",
        ],
    )?;
    cache(block)?;
    citation_setting(block, true)?;
    nullable_text(block, "context", false)?;
    nullable_text(block, "title", false)?;
    let source = &value["source"];
    match source["type"].as_str() {
        Some("base64") => validate_media(source, true),
        Some("url") => validate_url_source(source),
        Some("text") => {
            let source = object(source, &["type", "media_type", "data"])?;
            ensure!(
                text(source, "media_type")? == "text/plain",
                "unsupported document text media type"
            );
            text(source, "data")?;
            Ok(())
        }
        Some("content") => validate_document_content(source),
        Some("file") => unavailable_file(source),
        _ => anyhow::bail!("unsupported provider document source"),
    }
}

fn validate_document_content(value: &Value) -> Result<()> {
    let source = object(value, &["type", "content"])?;
    let content = source.get("content").context("document content missing")?;
    if content.is_string() {
        return Ok(());
    }
    for block in content
        .as_array()
        .context("document content must be a string or block array")?
    {
        match block["type"].as_str() {
            Some("text") => validate_text(block)?,
            Some("image") => validate_image(block)?,
            _ => anyhow::bail!("document content permits only text and image blocks"),
        }
    }
    Ok(())
}

fn validate_url_source(value: &Value) -> Result<()> {
    let source = object(value, &["type", "url"])?;
    let url = reqwest::Url::parse(text(source, "url")?)
        .map_err(|_| anyhow::anyhow!("provider source URL malformed"))?;
    ensure!(
        matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
        "provider source URL must be HTTP(S)"
    );
    Ok(())
}

fn unavailable_file(value: &Value) -> Result<()> {
    let source = object(value, &["type", "file_id"])?;
    ensure!(
        !text(source, "file_id")?.is_empty(),
        "provider file identifier missing"
    );
    anyhow::bail!("Claude source capability unavailable: provider file sources are not qualified")
}

fn validate_media(value: &Value, pdf: bool) -> Result<()> {
    let source = object(value, &["type", "data", "media_type"])?;
    let media = text(source, "media_type")?;
    ensure!(
        if pdf {
            media == "application/pdf"
        } else {
            matches!(
                media,
                "image/png" | "image/jpeg" | "image/gif" | "image/webp"
            )
        },
        "unsupported provider media type"
    );
    // measure() has already bounded encoded size before this allocation.
    let data = base64::engine::general_purpose::STANDARD
        .decode(text(source, "data")?)
        .map_err(|_| anyhow::anyhow!("provider media base64 malformed"))?;
    let signature = match media {
        "image/png" => data.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => data.starts_with(b"\xff\xd8\xff"),
        "image/gif" => data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a"),
        "image/webp" => data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP"),
        "application/pdf" => data.starts_with(b"%PDF-"),
        _ => false,
    };
    ensure!(
        signature,
        "provider media signature differs from declared media type"
    );
    Ok(())
}

fn validate_search(value: &Value) -> Result<()> {
    let block = object(
        value,
        &[
            "type",
            "source",
            "title",
            "content",
            "cache_control",
            "citations",
        ],
    )?;
    text(block, "source")?;
    text(block, "title")?;
    cache(block)?;
    citation_setting(block, false)?;
    let content = value["content"]
        .as_array()
        .context("search content must be a text block array")?;
    ensure!(
        !content.is_empty(),
        "search content must contain nonempty text"
    );
    for text in content {
        ensure!(
            text["type"] == "text" && text["text"].as_str().is_some_and(|value| !value.is_empty()),
            "search content must contain nonempty text"
        );
        validate_text(text)?;
    }
    Ok(())
}

pub(super) fn search_policy(value: &Value) -> Result<Option<bool>> {
    let Some(blocks) = value.as_array() else {
        return Ok(None);
    };
    if !blocks.iter().any(|block| block["type"] == "search_result") {
        return Ok(None);
    }
    let mut policy = None;
    for block in blocks {
        ensure!(
            block["type"] == "search_result",
            "Claude request compatibility: search results cannot mix with other content siblings"
        );
        let enabled =
            citation_setting(block.as_object().context("search result malformed")?, false)?;
        ensure!(
            policy.is_none_or(|previous| previous == enabled),
            "Claude request compatibility: search citation settings disagree"
        );
        policy = Some(enabled);
    }
    Ok(policy)
}

fn validate_citation(value: &Value) -> Result<()> {
    let (fields, start, end, index) = match value["type"].as_str() {
        Some("char_location") => (
            &[
                "type",
                "cited_text",
                "document_index",
                "document_title",
                "start_char_index",
                "end_char_index",
            ][..],
            "start_char_index",
            "end_char_index",
            "document_index",
        ),
        Some("page_location") => (
            &[
                "type",
                "cited_text",
                "document_index",
                "document_title",
                "start_page_number",
                "end_page_number",
            ][..],
            "start_page_number",
            "end_page_number",
            "document_index",
        ),
        Some("content_block_location") => (
            &[
                "type",
                "cited_text",
                "document_index",
                "document_title",
                "start_block_index",
                "end_block_index",
            ][..],
            "start_block_index",
            "end_block_index",
            "document_index",
        ),
        Some("search_result_location") => (
            &[
                "type",
                "cited_text",
                "search_result_index",
                "source",
                "title",
                "start_block_index",
                "end_block_index",
            ][..],
            "start_block_index",
            "end_block_index",
            "search_result_index",
        ),
        Some("web_search_result_location") => {
            let citation = object(
                value,
                &["type", "cited_text", "encrypted_index", "title", "url"],
            )?;
            for field in ["cited_text", "encrypted_index", "url"] {
                text(citation, field)?;
            }
            nullable_text(citation, "title", true)?;
            return Ok(());
        }
        _ => anyhow::bail!("unsupported text citation type"),
    };
    let citation = object(value, fields)?;
    text(citation, "cited_text")?;
    ensure!(
        value[index].as_u64().is_some(),
        "citation index must be nonnegative integer"
    );
    let start = value[start]
        .as_u64()
        .context("citation start index malformed")?;
    let end = value[end]
        .as_u64()
        .context("citation end index malformed")?;
    ensure!(start <= end, "citation index range reversed");
    if value["type"] == "search_result_location" {
        text(citation, "source")?;
        nullable_text(citation, "title", true)?;
    } else {
        nullable_text(citation, "document_title", true)?;
    }
    Ok(())
}

pub(crate) fn retained_search_policy(
    record: &crate::workflow::runtime::Record,
    facts: &super::PostToolFacts,
) -> Result<Option<bool>> {
    use super::PostDelivery;
    let Some(session_id) = source_session(&facts.representation)? else {
        return Ok(None);
    };
    let mut policy = None;
    for post in record
        .operations
        .iter()
        .filter_map(|operation| operation.tool_receipt.as_ref()?.plugin_lifecycle.as_ref())
    {
        if post.facts.operation == facts.operation
            || !(matches!(
                post.delivery,
                PostDelivery::Reserved | PostDelivery::Acknowledged
            ) || (post.correction_presentation
                == Some(super::CorrectionPresentation::ClaudeProviderBlocksV1)
                && matches!(
                    post.delivery,
                    PostDelivery::CorrectionReserved { .. }
                        | PostDelivery::CorrectionAcknowledged { .. }
                )))
        {
            continue;
        }
        let Some(previous_session) = source_session(&post.facts.representation)? else {
            continue;
        };
        if previous_session != session_id {
            continue;
        }
        if let Some(previous) = post
            .model_content
            .as_ref()
            .map(search_policy)
            .transpose()?
            .flatten()
        {
            ensure!(
                policy.is_none_or(|current| current == previous),
                "Claude request compatibility: retained search citation settings disagree"
            );
            policy = Some(previous);
        }
    }
    Ok(policy)
}

fn source_session(representation: &super::ToolRepresentation) -> Result<Option<&str>> {
    let super::ToolRepresentation::ClaudeMcp { source_input, .. } = representation else {
        return Ok(None);
    };
    source_input["session_id"]
        .as_str()
        .filter(|session| !session.is_empty())
        .map(Some)
        .context("Claude request compatibility: nonempty source session identifier required")
}

pub(crate) fn validate_release(
    record: &crate::workflow::runtime::Record,
    post: &super::LifecycleReceipt,
) -> Result<()> {
    let policy = retained_search_policy(record, &post.facts)?;
    if post.facts.representation.is_mcp()
        && let Some(value) = &post.model_content
    {
        validate(value, policy)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
