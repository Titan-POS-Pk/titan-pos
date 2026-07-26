//! Generated gRPC client code for cloud sync protocol.
//!
//! This module includes the Rust code generated from `proto/titan_sync.proto`.
//! It provides client stubs for communicating with the cloud API over gRPC.
//!
//! ## Services Available
//! - `AuthServiceClient` - Exchange API key for JWT, refresh tokens
//! - `SyncServiceClient` - Upload/download sync data
//! - `ConfigServiceClient` - Get/update store configuration  
//! - `NotificationServiceClient` - Real-time push notifications
//! - `HealthServiceClient` - Health checks

// `Notification::Payload` is a prost `oneof` whose variants differ in size by
// more than clippy's threshold. The shape is dictated by titan_sync.proto and
// the code is regenerated on every build, so it cannot be edited here.
#![allow(clippy::large_enum_variant)]

// Include the generated code from build.rs
tonic::include_proto!("titan.sync.v1");
