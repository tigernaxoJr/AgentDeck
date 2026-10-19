pub mod db;
pub mod dto;
pub mod error;

/// 取得單元列舉的 serde 標籤字串（例如 `QState::Asking` → `"asking"`），
/// 用於寫入 DB 欄位與回傳給前端。非字串序列化時回傳空字串。
pub fn enum_tag<T: serde::Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        _ => String::new(),
    }
}
