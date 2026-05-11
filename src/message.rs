use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Message {
    #[serde(rename = "event")]
    Event {
        event_id: String,
        event_name: String,
        timestamp_ns: u64,
        version: u8,
        process_id: u32,
        sequence_number: u64,
    },
    #[serde(rename = "session")]
    Session {
        session_id: String,
        session_type: String,
        timestamp_ns: u64,
        is_start: bool,
        version: u8,
        process_id: u32,
        sequence_number: u64,
    },
}

impl Message {
    pub fn to_msgpack(&self) -> Result<Vec<u8>, rmp_serde::encode::Error> {
        rmp_serde::to_vec_named(self)
    }

    pub fn from_msgpack(data: &[u8]) -> Result<Self, rmp_serde::decode::Error> {
        rmp_serde::from_slice(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_serialization_roundtrip() {
        let msg = Message::Event {
            event_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            event_name: "click".to_string(),
            timestamp_ns: 1000000,
            version: 1,
            process_id: 0,
            sequence_number: 42,
        };
        let serialized = msg.to_msgpack().unwrap();
        let deserialized = Message::from_msgpack(&serialized).unwrap();
        assert_eq!(msg, deserialized);
    }

    #[test]
    fn test_session_serialization_roundtrip() {
        let msg = Message::Session {
            session_id: "550e8400-e29b-41d4-a716-446655440001".to_string(),
            session_type: "browsing".to_string(),
            timestamp_ns: 2000000,
            is_start: true,
            version: 1,
            process_id: 1,
            sequence_number: 100,
        };
        let serialized = msg.to_msgpack().unwrap();
        let deserialized = Message::from_msgpack(&serialized).unwrap();
        assert_eq!(msg, deserialized);
    }
}
