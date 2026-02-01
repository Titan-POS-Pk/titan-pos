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

// Include the generated code from build.rs
tonic::include_proto!("titan.sync.v1");
