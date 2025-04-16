use std::error::Error;
mod network_layer;
mod adkg_layer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("Hello, world!");
    let n = 4; // Total nodes
    let t = 2; // Threshold (t+1 for reconstruction)
    let idx = 0u64;
    network_layer::run(n, t, idx).await
}
