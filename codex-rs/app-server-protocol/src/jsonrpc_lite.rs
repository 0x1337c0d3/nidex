//! We do not do true JSON-RPC 2.0, as we neither send nor expect the
//! "jsonrpc": "2.0" field.

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use ts_rs::TS;

pub const JSONRPC_VERSION: &str = "2.0";

/// A JSON-RPC request identifier — either a string or an integer.
///
/// `#[serde(untagged)]` is intentionally NOT used here. Untagged enum
/// deserialization inside serde's internally-tagged `ClientRequest` enum
/// (which uses `#[serde(tag = "method")]`) triggers a known serde limitation:
/// the error from the first failed variant attempt ("invalid type: integer,
/// expected a string") leaks out instead of being silently retried. A manual
/// `Visitor` with `deserialize_any` avoids this.
#[derive(Debug, Clone, PartialEq, Serialize, Hash, Eq, JsonSchema, TS)]
#[serde(untagged)]
pub enum RequestId {
    String(String),
    #[ts(type = "number")]
    Integer(i64),
}

impl<'de> Deserialize<'de> for RequestId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = RequestId;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a string or integer request id")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<RequestId, E> {
                Ok(RequestId::String(v.to_owned()))
            }
            fn visit_string<E: serde::de::Error>(self, v: String) -> std::result::Result<RequestId, E> {
                Ok(RequestId::String(v))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<RequestId, E> {
                Ok(RequestId::Integer(v))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<RequestId, E> {
                Ok(RequestId::Integer(v as i64))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

pub type Result = serde_json::Value;

/// Refers to any valid JSON-RPC object that can be decoded off the wire, or encoded to be sent.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, JsonSchema, TS)]
#[serde(untagged)]
pub enum JSONRPCMessage {
    Request(JSONRPCRequest),
    Notification(JSONRPCNotification),
    Response(JSONRPCResponse),
    Error(JSONRPCError),
}

/// A request that expects a response.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, JsonSchema, TS)]
pub struct JSONRPCRequest {
    pub id: RequestId,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub params: Option<serde_json::Value>,
}

/// A notification which does not expect a response.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, JsonSchema, TS)]
pub struct JSONRPCNotification {
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub params: Option<serde_json::Value>,
}

/// A successful (non-error) response to a request.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, JsonSchema, TS)]
pub struct JSONRPCResponse {
    pub id: RequestId,
    pub result: Result,
}

/// A response to a request that indicates an error occurred.
/// `id` is optional because Zed (and per JSON-RPC spec) sends error responses
/// without an id when the error is for a notification (which has no id).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, JsonSchema, TS)]
pub struct JSONRPCError {
    pub error: JSONRPCErrorError,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<RequestId>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, JsonSchema, TS)]
pub struct JSONRPCErrorError {
    pub code: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub data: Option<serde_json::Value>,
    pub message: String,
}
