// build.rs
// Compiles the sglang-lite proto when the sglang-lite-grpc feature is enabled.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The build-dep tonic-build is only present when sglang-lite-grpc feature is active.
    #[cfg(feature = "sglang-lite-grpc")]
    {
        let proto = "proto/sglang_lite.proto";
        println!("cargo:rerun-if-changed={}", proto);

        tonic_build::configure()
            .build_server(false) // client only
            .compile_protos(&[proto], &["proto"])?;
    }
    Ok(())
}
