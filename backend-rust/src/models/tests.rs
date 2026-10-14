#[cfg(test)]
mod tests {
    use crate::models::protocol::*;
    use chrono::Utc;
    use uuid::Uuid;

    #[test]
    fn test_task_state_serialization() {
        let state = TaskState::Queued;
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, "\"QUEUED\"");

        let de: TaskState = serde_json::from_str(&json).unwrap();
        assert_eq!(de, TaskState::Queued);
    }

    #[test]
    fn test_server_event_tagged_union() {
        let task_id = Uuid::new_v4();
        let event = ServerEvent::TaskStateChanged {
            task_id,
            old_state: Some(TaskState::Queued),
            new_state: TaskState::Running,
            exit_code: None,
            timestamp: Utc::now(),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"TASK_STATE_CHANGED\""));
        assert!(json.contains("\"payload\":{"));
        assert!(json.contains("\"newState\":\"RUNNING\""));
    }

    #[test]
    fn test_client_command_deserialization() {
        let task_id = Uuid::new_v4();
        let raw = format!(
            r#"{{"type":"SUBSCRIBE_TASK","payload":{{"taskId":"{}"}}}}"#,
            task_id
        );
        let cmd: ClientCommand = serde_json::from_str(&raw).unwrap();
        match cmd {
            ClientCommand::SubscribeTask { task_id: id } => assert_eq!(id, task_id),
            _ => panic!("Unexpected command parsed"),
        }
    }
}
