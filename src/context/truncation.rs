/// Truncate a variable value string if it exceeds `max_len`, appending an ellipsis summary.
pub fn truncate_value(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_string();
    }
    format!("{}... [truncated, {} total chars]", &value[..max_len], value.len())
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
