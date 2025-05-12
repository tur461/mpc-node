
#[allow(dead_code)]
#[allow(unused_imports)]
#[allow(non_snake_case)]
#[allow(unused_variables)]

use std::error::Error;
use std::fs::File;
use rand::{thread_rng, Rng};
use tracing::{instrument::WithSubscriber, level_filters::LevelFilter};
use tracing_appender::{non_blocking, rolling};
use tracing_subscriber::{fmt, EnvFilter, prelude::*};
use mpc_node::{
    network::NetworkLayer,
    dkg::DKGNode,
    consensus::ConsensusNode,
    commands::CommandProcessor,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let rand_num: u32 = thread_rng().gen_range(1000..9999);
    // let filename = format!("logs/mpcn-{}.log", rand_num);
    // let file_appender = rolling::daily("logs", "mpcn.log");
    // let (file_writer, _guard) = tracing_appender::non_blocking(file_appender);
    let file_appender = tracing_appender::rolling::never("logs", format!("mpcn-{}.log", rand_num));
    // let file = File::create(&filename).expect("Failed to create log file");
    let (writer, guard) = non_blocking(file_appender);
    let file_layer = fmt::layer::<_>()
        .with_writer(writer)
        .with_target(false)
        .with_level(true)
        .with_thread_ids(false)
        .with_thread_names(false);
    
    let stdout_layer = fmt::layer::<_>()
        .with_writer(std::io::stdout)
        .with_target(false)
        .with_level(true)
        .with_thread_ids(false)
        .with_thread_names(false);
    let stderr_layer = fmt::layer::<_>()
        .with_writer(std::io::stderr)
        .with_target(false)
        .with_level(true)
        .with_thread_ids(false)
        .with_thread_names(false);

    let x =  EnvFilter::new("info");
    let y = EnvFilter::builder().with_default_directive(LevelFilter::INFO.into()).parse("")?;

    let _ = tracing_subscriber::registry()
    .with(y)
    .with(file_layer)
    .with(stdout_layer)
    .with(stderr_layer)
    .init();


    let threshold = 3;
    let total = 5;

    let mut network = NetworkLayer::new().await?;
    
    let dkg_node = DKGNode::new(
        network.get_local_peer_id().to_string(),
        threshold,
        total,
        network.get_msg_tx(),
    );
    
    let consensus_node = ConsensusNode::new();
    let command_processor = CommandProcessor::new();
    
    network.set_dkg_node(dkg_node);
    network.set_consensus_node(consensus_node);
    network.set_command_processor(command_processor);
    
    // Start the network
    network.start().await?;

    Ok(())
}

#[cfg(test)]
mod test {
    use tracing_subscriber::fmt::format;

    
    fn call_once<F>(f: F) where F: FnOnce() {
        f();
    }

    fn call_borrow<F>(f: F) where F: Fn() {
        f();
        f();
    }

    fn call_borrow_mut<F: FnMut()>(mut f: F) {
        f();
        f();
    }

    
    #[test]
    fn test_it() {
        let mut ct = 100;
        let mut s = String::from("value");
        let inc = || {
            ct += 1;
            let a = ct;
        };
        call_borrow_mut(inc);
        assert_eq!(ct, 102);
        let inc = || println!("ct {}", ct);
        call_borrow(inc);
        assert_eq!(ct, 102);
        let inc = move || { for i in 0..10 {
            println!("{s}");
        } };
        call_borrow(inc);
        assert_eq!(ct, 102);
  
    }
}