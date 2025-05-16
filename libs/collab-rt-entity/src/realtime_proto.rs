#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct HttpRealtimeMessage {
  #[prost(string, tag = "1")]
  pub device_id: ::prost::alloc::string::String,
  #[prost(bytes = "vec", tag = "2")]
  pub payload: ::prost::alloc::vec::Vec<u8>,
}
