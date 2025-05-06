fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Compile the proto file
    // Note: Make sure service.proto exists in your project root or specify the correct path
    tonic_build::compile_protos("service.proto")?;
    Ok(())
}
