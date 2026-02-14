use chrono::Local;
use serde_json::Value;
use std::fs::OpenOptions;
use std::io::Write;

/// Append a single structured event as a JSON line to the per-user debug log.
///
/// Location (Linux/macOS/Windows via dirs): <data_dir>/mylm/toolcall.jsonl
pub fn append_jsonl(event: &Value) {
    let Some(data_dir) = dirs::data_dir() else {
        return;
    };

    let dir = data_dir.join("mylm");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("toolcall.jsonl");

    let mut obj = match event.as_object() {
        Some(map) => map.clone(),
        None => {
            let mut m = serde_json::Map::new();
            m.insert("event".to_string(), event.clone());
            m
        }
    };

    obj.insert(
        "ts".to_string(),
        Value::String(Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string()),
    );

    if let Ok(line) = serde_json::to_string(&Value::Object(obj)) {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{}", line);
        }
    }
}

pub fn append_jsonl_owned(event: Value) {
    append_jsonl(&event)
}

