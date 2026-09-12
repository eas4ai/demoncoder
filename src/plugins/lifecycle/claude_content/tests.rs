use super::*;
use serde_json::json;

fn search(enabled: Option<bool>) -> Value {
    let mut result = json!({"type":"search_result","source":"https://example.test/item","title":"result","content":[{"type":"text","text":"content"}]});
    if let Some(enabled) = enabled {
        result["citations"] = json!({"enabled":enabled});
    }
    result
}

#[test]
fn metadata_is_typed_and_source_values_are_not_rewritten() {
    let citations = [
        json!({"type":"char_location","cited_text":"c","document_index":0,"document_title":null,"start_char_index":0,"end_char_index":1}),
        json!({"type":"page_location","cited_text":"c","document_index":0,"document_title":"title","start_page_number":1,"end_page_number":2}),
        json!({"type":"content_block_location","cited_text":"c","document_index":0,"document_title":null,"start_block_index":0,"end_block_index":1}),
        json!({"type":"search_result_location","cited_text":"c","search_result_index":0,"source":"source","title":null,"start_block_index":0,"end_block_index":1}),
        json!({"type":"web_search_result_location","cited_text":"c","encrypted_index":"opaque","url":"https://example.test/","title":null}),
    ];
    for citation in citations {
        let value = json!([{"type":"text","text":"text","cache_control":{"type":"ephemeral","ttl":"1h"},"citations":[citation]}]);
        let original = value.clone();
        validate(&value, None).unwrap();
        assert_eq!(value, original);
    }
    for value in [
        json!(""),
        json!([]),
        json!([{"type":"text","text":"","cache_control":null,"citations":null}]),
        json!([{"type":"document","source":{"type":"content","content":"nested text"},"title":null,"context":null,"citations":null}]),
    ] {
        validate(&value, None).unwrap();
    }
}

#[test]
fn malformed_nested_shapes_and_nonprovider_blocks_remain_rejected() {
    let cases = [
        json!({"type":"text","text":"not an array"}),
        json!([{"type":"text","text":3}]),
        json!([{"type":"text","text":"x","extra":"field"}]),
        json!([{"type":"text","text":"x","cache_control":{"type":"ephemeral","ttl":"forever"}}]),
        json!([{"type":"text","text":"x","citations":[{"type":"page_location","cited_text":"x","document_index":0,"document_title":null,"start_page_number":2,"end_page_number":1}]}]),
        json!([{"type":"image","data":"iVBORw0KGgo=","mimeType":"image/png"}]),
        json!([{"type":"image","source":{"type":"base64","media_type":"image/png","data":"not base64"}}]),
        json!([{"type":"image","source":{"type":"base64","media_type":"image/png","data":"bm90IGEgcG5n"}}]),
        json!([{"type":"image","source":{"type":"url","url":"file:///secret"}}]),
        json!([{"type":"image","source":{"type":"url","url":"relative/path"}}]),
        json!([{"type":"document","source":{"type":"base64","media_type":"text/plain","data":"JVBERi0="}}]),
        json!([{"type":"document","source":{"type":"text","media_type":"text/plain","data":false}}]),
        json!([{"type":"document","source":{"type":"content","content":[{"type":"document","source":{"type":"text","media_type":"text/plain","data":"x"}}]}}]),
        json!([{"type":"resource","resource":{"uri":"file:///secret","text":"x"}}]),
    ];
    for value in cases {
        assert!(validate(&value, None).is_err(), "{value}");
    }
}

#[test]
fn source_capability_holds_are_distinct_from_malformed_content() {
    for value in [
        json!([{"type":"tool_reference","tool_name":"mcp__demoncoder__read"}]),
        json!([{"type":"image","source":{"type":"file","file_id":"existing-file"}}]),
        json!([{"type":"document","source":{"type":"file","file_id":"existing-file"}}]),
    ] {
        assert!(
            validate(&value, None)
                .unwrap_err()
                .to_string()
                .contains("source capability unavailable")
        );
    }
    let malformed = json!([{"type":"tool_reference","tool_name":false}]);
    assert!(
        !validate(&malformed, None)
            .unwrap_err()
            .to_string()
            .contains("source capability unavailable")
    );
}

#[test]
fn search_siblings_and_retained_citation_policy_must_agree() {
    validate(&json!([search(None), search(Some(false))]), Some(false)).unwrap();
    validate(&json!([search(Some(true))]), None).unwrap();
    assert!(validate(&json!([search(Some(true))]), Some(false)).is_err());
    assert!(validate(&json!([search(None), search(Some(true))]), None).is_err());
    assert!(
        validate(
            &json!([search(None), {"type":"text","text":"attribution belongs outside this array"}]),
            None
        )
        .is_err()
    );
    let mut empty = search(None);
    empty["content"][0]["text"] = json!("");
    assert!(validate(&json!([empty]), None).is_err());
}

#[test]
fn existing_wire_limits_apply_before_shape_or_media_allocation() {
    let oversized =
        json!([{"type":"text","text":"x".repeat(super::super::super::wire::MAX_WIRE_BYTES)}]);
    assert!(validate(&oversized, None).is_err());
    let mut deep = json!("x");
    for _ in 0..=super::super::super::wire::MAX_WIRE_DEPTH {
        deep = json!([deep]);
    }
    assert!(validate(&deep, None).is_err());
    let nodes = Value::Array(vec![Value::Null; super::super::super::wire::MAX_WIRE_NODES]);
    assert!(validate(&nodes, None).is_err());
}
