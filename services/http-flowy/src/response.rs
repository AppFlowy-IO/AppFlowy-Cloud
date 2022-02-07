use crate::errors::{ServerError};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::{convert::TryInto, fmt::Debug};

#[derive(Debug, Serialize, Deserialize)]
pub struct FlowyResponse {
    pub data: Bytes,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ServerError>,
}

impl FlowyResponse {
    pub fn new(data: Bytes, error: Option<ServerError>) -> Self {
        FlowyResponse { data, error }
    }

    pub fn success() -> Self {
        Self::new(Bytes::new(), None)
    }

    pub fn data<T: TryInto<Bytes, Error = protobuf::ProtobufError>>(mut self, data: T) -> Result<Self, ServerError> {
        let bytes: Bytes = data.try_into()?;
        self.data = bytes;
        Ok(self)
    }

    pub fn pb<T: ::protobuf::Message>(mut self, data: T) -> Result<Self, ServerError> {
        let bytes: Bytes = Bytes::from(data.write_to_bytes()?);
        self.data = bytes;
        Ok(self)
    }
}

impl std::convert::From<protobuf::ProtobufError> for ServerError {
    fn from(e: protobuf::ProtobufError) -> Self {
        ServerError::internal().context(e)
    }
}

impl std::convert::From<serde_json::Error> for ServerError {
    fn from(e: serde_json::Error) -> Self {
        ServerError::internal().context(e)
    }
}

impl std::convert::From<anyhow::Error> for ServerError {
    fn from(error: anyhow::Error) -> Self {
        ServerError::internal().context(error)
    }
}


impl std::convert::From<uuid::Error> for ServerError {
    fn from(e: uuid::Error) -> Self {
        ServerError::internal().context(e)
    }
}


impl std::convert::From<&ServerError> for FlowyResponse {
    fn from(error: &ServerError) -> Self {
        FlowyResponse {
            data: Bytes::from(vec![]),
            error: Some(error.clone()),
        }
    }
}
