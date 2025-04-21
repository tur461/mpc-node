

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .out_dir("src/protos")
        .compile_protos(
            &["proto/intent.proto"],
            &["proto"],
        )?;
    Ok(())
}