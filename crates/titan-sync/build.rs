//! Build script for titan-sync
//!
//! This script compiles the gRPC protocol definitions from `proto/titan_sync.proto`
//! into Rust code that can be used by the sync client to communicate with the
//! cloud API.
//!
//! ## Generated Code
//! The proto compilation generates:
//! - Client stubs for all services (AuthService, SyncService, etc.)
//! - Message types matching the .proto definitions
//! - Serialization/deserialization code via prost

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Path to the proto file (relative to crate root)
    let proto_file = "../../proto/titan_sync.proto";
    
    // Only recompile if the proto file changes
    println!("cargo:rerun-if-changed={}", proto_file);
    
    // Configure tonic-build for client generation only
    // We don't need server code in titan-sync - that's in cloud-api
    tonic_build::configure()
        // Don't generate server code - titan-sync is a client
        .build_server(false)
        // Generate client code for calling the cloud API
        .build_client(true)
        // Compile the proto file
        .compile_protos(
            &[proto_file],
            &["../../proto"], // Include directory for imports
        )?;
    
    Ok(())
}
