use ethers::{
    prelude::*,
    providers::{Provider, Ws},
    types::Address,
};
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct BlockchainClient {
    provider: Arc<Provider<Ws>>,
    wallet: LocalWallet,
    latest_block: RwLock<u64>,
}

impl BlockchainClient {
    pub async fn new(ws_url: &str, private_key: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let provider = Provider::<Ws>::connect(ws_url).await?;
        let wallet = private_key.parse::<LocalWallet>()?;
        
        Ok(Self {
            provider: Arc::new(provider),
            wallet,
            latest_block: RwLock::new(0),
        })
    }

    pub async fn sync_blocks(&self) -> Result<(), Box<dyn std::error::Error>> {
        let current_block = self.provider.get_block_number().await?;
        *self.latest_block.write().await = current_block.as_u64();
        Ok(())
    }

    pub async fn send_transaction(
        &self,
        to: Address,
        data: Vec<u8>,
    ) -> Result<H256, Box<dyn std::error::Error>> {
        let tx = TransactionRequest::new()
            .to(to)
            .data(data)
            .from(self.wallet.address());

        let tx_hash = self.provider
            .send_transaction(tx, None)
            .await?
            .tx_hash();

        Ok(tx_hash)
    }
}