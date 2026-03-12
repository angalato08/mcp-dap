/// Truncate a variable value string if it exceeds `max_len`, appending an ellipsis summary.
pub fn truncate_value(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_string();
    }
    let boundary = value.floor_char_boundary(max_len);
    format!(
        "{}... [truncated, {} total chars]",
        &value[..boundary],
        value.len()
    )
}

/// Summarize a JSON array that is too large for LLM context.
pub fn summarize_array(value: &serde_json::Value, max_items: usize) -> serde_json::Value {
    let Some(arr) = value.as_array() else {
        return value.clone();
    };

    if arr.len() <= max_items {
        return value.clone();
    }

    let preview: Vec<serde_json::Value> = arr.iter().take(max_items).cloned().collect();
    serde_json::json!({
        "_summary": format!("[Array of {} items — showing first {}]", arr.len(), max_items),
        "items": preview,
    })
}

/// Summarize a JSON object that has too many keys for LLM context.
pub fn summarize_object(value: &serde_json::Value, max_keys: usize) -> serde_json::Value {
    let Some(obj) = value.as_object() else {
        return value.clone();
    };

    if obj.len() <= max_keys {
        return value.clone();
    }

    let preview: serde_json::Map<String, serde_json::Value> =
        obj.iter().take(max_keys).map(|(k, v)| (k.clone(), v.clone())).collect();

    let mut result = serde_json::Value::Object(preview);
    result["_summary"] = serde_json::json!(
        format!("[Object with {} keys — showing first {}]", obj.len(), max_keys)
    );
    result
}

/// Recursively truncate JSON values beyond `max_depth`, replacing deep
/// containers with summary strings.
pub fn truncate_nested(value: &serde_json::Value, max_depth: usize) -> serde_json::Value {
    match value {
        serde_json::Value::Array(arr) => {
            if max_depth == 0 {
                serde_json::Value::String(format!(
                    "[Array with {} items — depth limit reached]",
                    arr.len()
                ))
            } else {
                serde_json::Value::Array(
                    arr.iter()
                        .map(|v| truncate_nested(v, max_depth - 1))
                        .collect(),
                )
            }
        }
        serde_json::Value::Object(obj) => {
            if max_depth == 0 {
                serde_json::Value::String(format!(
                    "[Object with {} keys — depth limit reached]",
                    obj.len()
                ))
            } else {
                serde_json::Value::Object(
                    obj.iter()
                        .map(|(k, v)| (k.clone(), truncate_nested(v, max_depth - 1)))
                        .collect(),
                )
            }
        }
        _ => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn short_string_unchanged() {
        assert_eq!(truncate_value("hello", 10), "hello");
    }

    #[test]
    fn exact_length_unchanged() {
        assert_eq!(truncate_value("hello", 5), "hello");
    }

    #[test]
    fn long_string_truncated() {
        let result = truncate_value("hello world", 5);
        assert!(result.starts_with("hello"));
        assert!(result.contains("truncated"));
        assert!(result.contains("11 total chars"));
    }

    #[test]
    fn empty_string() {
        assert_eq!(truncate_value("", 10), "");
    }

    #[test]
    fn zero_max_len() {
        let result = truncate_value("hello", 0);
        assert!(result.starts_with("..."));
        assert!(result.contains("truncated"));
    }

    #[test]
    fn utf8_boundary_no_panic() {
        // 🌍 is 4 bytes; cutting at byte 1 would panic without floor_char_boundary
        let emoji = "🌍🌍🌍";
        let result = truncate_value(emoji, 1);
        assert!(result.contains("truncated"));
        // Should not include a partial emoji
        assert!(!result.contains('\u{FFFD}'));
    }

    #[test]
    fn summarize_array_under_limit() {
        let arr = json!([1, 2, 3]);
        assert_eq!(summarize_array(&arr, 5), arr);
    }

    #[test]
    fn summarize_array_over_limit() {
        let arr = json!([1, 2, 3, 4, 5]);
        let result = summarize_array(&arr, 2);
        assert!(result["_summary"].as_str().unwrap().contains("5 items"));
        assert_eq!(result["items"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn summarize_array_empty() {
        let arr = json!([]);
        assert_eq!(summarize_array(&arr, 5), arr);
    }

    #[test]
    fn summarize_array_non_array_passthrough() {
        let val = json!("not an array");
        assert_eq!(summarize_array(&val, 5), val);
    }

    #[test]
    fn summarize_object_under_limit() {
        let obj = json!({"a": 1, "b": 2});
        assert_eq!(summarize_object(&obj, 5), obj);
    }

    #[test]
    fn summarize_object_over_limit() {
        let obj = json!({"a": 1, "b": 2, "c": 3, "d": 4, "e": 5});
        let result = summarize_object(&obj, 2);
        assert!(result["_summary"].as_str().unwrap().contains("5 keys"));
        // Should have 2 original keys + _summary
        assert_eq!(result.as_object().unwrap().len(), 3);
    }

    #[test]
    fn summarize_object_empty() {
        let obj = json!({});
        assert_eq!(summarize_object(&obj, 5), obj);
    }

    #[test]
    fn summarize_object_non_object_passthrough() {
        let val = json!(42);
        assert_eq!(summarize_object(&val, 5), val);
    }

    #[test]
    fn truncate_nested_shallow_unchanged() {
        let val = json!({"a": 1, "b": [2, 3]});
        assert_eq!(truncate_nested(&val, 3), val);
    }

    #[test]
    fn truncate_nested_deep_replaced() {
        let val = json!({"a": {"b": {"c": {"d": 1}}}});
        let result = truncate_nested(&val, 2);
        // max_depth=2: root(2) → a's value(1) → b's value(0) → replaced
        let deep = &result["a"]["b"];
        assert!(deep.as_str().unwrap().contains("depth limit reached"));
    }

    #[test]
    fn truncate_nested_scalar_passthrough() {
        assert_eq!(truncate_nested(&json!(42), 0), json!(42));
        assert_eq!(truncate_nested(&json!("hello"), 0), json!("hello"));
        assert_eq!(truncate_nested(&json!(null), 0), json!(null));
    }

    #[test]
    fn truncate_nested_zero_depth_replaces_root_containers() {
        let arr = json!([1, 2, 3]);
        let result = truncate_nested(&arr, 0);
        assert!(result.as_str().unwrap().contains("Array with 3 items"));

        let obj = json!({"a": 1});
        let result = truncate_nested(&obj, 0);
        assert!(result.as_str().unwrap().contains("Object with 1 keys"));
    }
}
