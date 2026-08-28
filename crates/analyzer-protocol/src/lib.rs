//! Versioned NDJSON messages exchanged between Rust and Python workers.

use analyzer_domain::{JobId, RequestId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Hello {
    pub protocol_version: u32,
    #[serde(rename = "type")]
    pub message_type: HelloType,
    pub worker: String,
    pub worker_version: String,
    pub capabilities: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum HelloType {
    #[serde(rename = "hello")]
    Hello,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Request {
    pub protocol_version: u32,
    #[serde(rename = "type")]
    pub message_type: RequestType,
    pub request_id: RequestId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<JobId>,
    pub method: Method,
    #[serde(default)]
    pub params: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum RequestType {
    #[serde(rename = "request")]
    Request,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    Ping,
    InspectCapabilities,
    Shutdown,
    Cancel,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Response {
    pub protocol_version: u32,
    #[serde(rename = "type")]
    pub message_type: ResponseType,
    pub request_id: RequestId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<JobId>,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<WorkerError>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum ResponseType {
    #[serde(rename = "response")]
    Response,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct WorkerError {
    pub code: String,
    pub message: String,
    pub stage: String,
    pub recoverable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub technical_detail: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Event {
    pub protocol_version: u32,
    #[serde(rename = "type")]
    pub message_type: EventType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<JobId>,
    pub event: String,
    #[serde(default)]
    pub data: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum EventType {
    #[serde(rename = "event")]
    Event,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum WorkerMessage {
    Response(Response),
    Event(Event),
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("unsupported protocol version {received}; expected {expected}")]
    IncompatibleVersion { received: u32, expected: u32 },
}

pub fn validate_version(version: u32) -> Result<(), ProtocolError> {
    if version == PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(ProtocolError::IncompatibleVersion {
            received: version,
            expected: PROTOCOL_VERSION,
        })
    }
}

impl Request {
    #[must_use]
    pub fn new(method: Method) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            message_type: RequestType::Request,
            request_id: RequestId::new(),
            job_id: None,
            method,
            params: Value::Object(Default::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_a_request() {
        let request = Request::new(Method::Ping);
        let value = serde_json::to_value(&request).expect("request must serialize");

        assert_eq!(value["protocol_version"], PROTOCOL_VERSION);
        assert_eq!(value["type"], "request");
        assert_eq!(value["method"], "ping");
    }

    #[test]
    fn parses_a_response() {
        let response: WorkerMessage = serde_json::from_value(json!({
            "protocol_version": 1,
            "type": "response",
            "request_id": "req-123",
            "ok": true,
            "result": {"pong": true}
        }))
        .expect("response must parse");

        assert!(matches!(
            response,
            WorkerMessage::Response(Response { ok: true, .. })
        ));
    }

    #[test]
    fn parses_an_event() {
        let event: WorkerMessage = serde_json::from_value(json!({
            "protocol_version": 1,
            "type": "event",
            "event": "ready",
            "data": {}
        }))
        .expect("event must parse");

        assert!(matches!(event, WorkerMessage::Event(Event { event, .. }) if event == "ready"));
    }

    #[test]
    fn rejects_an_incompatible_protocol() {
        let error = validate_version(PROTOCOL_VERSION + 1).expect_err("version must be rejected");

        assert!(matches!(error, ProtocolError::IncompatibleVersion { .. }));
    }
}
