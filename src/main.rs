use std::error::Error;
mod network_layer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("Hello, world!");
    network_layer::run().await
}
