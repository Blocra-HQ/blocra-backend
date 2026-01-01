use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};

use crate::services::alchemy::AlchemyService;

#[derive(Debug, Deserialize)]
pub struct FetchTransactionsRequest {
    pub contract_address: String,
    pub chain: String,
    pub from_block: Option<String>,
    pub to_block: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FetchTransactionsResponse {
    pub transactions: Vec<crate::services::alchemy::Transaction>,
    pub count: usize,
    pub chain: String,
    pub block_range: BlockRange,
}

#[derive(Debug, Serialize)]
pub struct BlockRange {
    pub from: String,
    pub to: String,
}

/// POST /api/contract/transactions
/// Fetch transactions for a contract using Alchemy API
pub async fn fetch_transactions(
    req: web::Json<FetchTransactionsRequest>,
    alchemy: web::Data<AlchemyService>,
) -> HttpResponse {
    log::info!(
        "Fetching transactions for contract {} on chain {}",
        req.contract_address,
        req.chain
    );

    // Validate chain support
    if !AlchemyService::is_supported(&req.chain) {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": format!("Chain '{}' is not supported by Alchemy. Supported chains: base, ethereum, arbitrum, optimism, polygon", req.chain)
        }));
    }

    // Get Alchemy chain ID
    let alchemy_chain = AlchemyService::get_alchemy_chain_id(&req.chain);

    // Default block range: last 100,000 blocks
    let from_block = req.from_block.clone().unwrap_or_else(|| "0x0".to_string());
    let to_block = req.to_block.clone().unwrap_or_else(|| "latest".to_string());

    // Fetch transactions
    match alchemy
        .fetch_transactions(&req.contract_address, &alchemy_chain, &from_block, &to_block)
        .await
    {
        Ok(transactions) => {
            let count = transactions.len();
            log::info!("Successfully fetched {} transactions", count);

            HttpResponse::Ok().json(FetchTransactionsResponse {
                transactions,
                count,
                chain: req.chain.clone(),
                block_range: BlockRange {
                    from: from_block,
                    to: to_block,
                },
            })
        }
        Err(e) => {
            log::error!("Failed to fetch transactions: {}", e);
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("Failed to fetch transactions: {}", e)
            }))
        }
    }
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/contract")
            .route("/transactions", web::post().to(fetch_transactions))
    );
}
