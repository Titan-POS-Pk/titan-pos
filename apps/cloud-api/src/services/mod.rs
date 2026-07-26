//! gRPC service implementations.
//!
//! This module contains all the gRPC service implementations for the Cloud API.

// Every method in these modules is either a `tonic` service-trait method or a
// helper called by one, so the error type is fixed at `tonic::Status` (~176
// bytes, over clippy's 128-byte threshold). Boxing it in the private helpers
// while the trait methods keep returning it bare would add a conversion at
// every call site and hide nothing.
#![allow(clippy::result_large_err)]

pub mod auth_service;
pub mod config_service;
pub mod health_service;
pub mod notification_service;
pub mod sync_service;
