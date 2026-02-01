//! Build script for compiling Protocol Buffer definitions.
//!
//! This script compiles the .proto files into Rust code using tonic-build.
//! The generated code is placed in `$OUT_DIR` and included via `tonic::include_proto!`.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Tell Cargo to rerun this build script if the proto file changes
    println!("cargo:rerun-if-changed=../../proto/titan_sync.proto");
    println!("cargo:rerun-if-changed=../../proto");
    
    // Compile the proto files
    // Generated code goes to $OUT_DIR which is then included via include_proto!
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(
            &["../../proto/titan_sync.proto"],
            &["../../proto"],
        )?;

    Ok(())
}
