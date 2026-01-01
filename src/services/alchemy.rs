use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AlchemyTransfer {
    #[serde(rename = "blockNum")]
    pub block_num: String,
    pub hash: String,
    pub from: String,
    pub to: Option<String>,
    pub value: Option<f64>,
    pub asset: Option<String>,
    pub category: String,
    #[serde(rename = "rawContract")]
    pub raw_contract: RawContract,
    pub metadata: TransferMetadata,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RawContract {
    pub value: Option<String>,
    pub address: Option<String>,
    pub decimal: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TransferMetadata {
    #[serde(rename = "blockTimestamp")]
    pub block_timestamp: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Transaction {
    pub hash: String,
    pub block_number: u64,
    pub from: String,
    pub to: String,
    pub value: String,
    pub method_name: String,
    pub timestamp: i64,
}

#[derive(Debug, Serialize)]
struct AlchemyRequest {
    jsonrpc: String,
    method: String,
    params: Vec<AlchemyParams>,
    id: u32,
}

#[derive(Debug, Serialize)]
struct AlchemyParams {
    #[serde(rename = "fromBlock")]
    from_block: String,
    #[serde(rename = "toBlock")]
    to_block: String,
    category: Vec<String>,
    #[serde(rename = "withMetadata")]
    with_metadata: bool,
    #[serde(rename = "excludeZeroValue")]
    exclude_zero_value: bool,
    #[serde(rename = "maxCount")]
    max_count: String,
    #[serde(rename = "fromAddress", skip_serializing_if = "Option::is_none")]
    from_address: Option<String>,
    #[serde(rename = "toAddress", skip_serializing_if = "Option::is_none")]
    to_address: Option<String>,
    #[serde(rename = "pageKey", skip_serializing_if = "Option::is_none")]
    page_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AlchemyResponse {
    result: Option<AlchemyResult>,
    error: Option<AlchemyError>,
}

#[derive(Debug, Deserialize)]
struct AlchemyResult {
    transfers: Vec<AlchemyTransfer>,
    #[serde(rename = "pageKey")]
    page_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AlchemyError {
    message: String,
}

#[derive(Clone)]
pub struct AlchemyService {
    api_key: String,
    client: reqwest::Client,
}

impl AlchemyService {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }

    /// Fetch all transactions for a contract address using Alchemy's indexed API
    /// INSTANT: Returns results in 100-500ms (like Dune Analytics)
    pub async fn fetch_transactions(
        &self,
        contract_address: &str,
        chain: &str,
        from_block: &str,
        to_block: &str,
    ) -> Result<Vec<Transaction>, Box<dyn std::error::Error>> {
        log::info!(
            "Fetching transactions for {} on {} (blocks {} to {})",
            contract_address,
            chain,
            from_block,
            to_block
        );

        // Fetch outgoing transactions
        let outgoing = self
            .fetch_asset_transfers(
                contract_address,
                Some(contract_address),
                None,
                from_block,
                to_block,
                chain,
                "outgoing",
            )
            .await?;

        // Fetch incoming transactions
        let incoming = self
            .fetch_asset_transfers(
                contract_address,
                None,
                Some(contract_address),
                from_block,
                to_block,
                chain,
                "incoming",
            )
            .await?;

        // Combine and deduplicate
        let mut all_transfers = outgoing;
        all_transfers.extend(incoming);

        let mut unique_hashes = HashSet::new();
        let mut transactions = Vec::new();

        for transfer in all_transfers {
            if unique_hashes.contains(&transfer.hash) {
                continue;
            }
            unique_hashes.insert(transfer.hash.clone());

            let block_number = u64::from_str_radix(&transfer.block_num.trim_start_matches("0x"), 16)
                .unwrap_or(0);

            let timestamp = chrono::DateTime::parse_from_rfc3339(&transfer.metadata.block_timestamp)
                .map(|dt| dt.timestamp())
                .unwrap_or(0);

            transactions.push(Transaction {
                hash: transfer.hash.clone(),
                block_number,
                from: transfer.from.clone(),
                to: transfer.to.clone().unwrap_or_default(),
                value: transfer.raw_contract.value.clone().unwrap_or_else(|| "0x0".to_string()),
                method_name: Self::categorize_transfer(&transfer.category),
                timestamp,
            });
        }

        log::info!("Found {} transactions", transactions.len());
        Ok(transactions)
    }

    /// Fetch asset transfers using Alchemy API
    async fn fetch_asset_transfers(
        &self,
        _contract_address: &str,
        from_address: Option<&str>,
        to_address: Option<&str>,
        from_block: &str,
        to_block: &str,
        chain: &str,
        direction: &str,
    ) -> Result<Vec<AlchemyTransfer>, Box<dyn std::error::Error>> {
        let alchemy_url = format!("https://{}-mainnet.g.alchemy.com/v2/{}", chain, self.api_key);

        // Base chain doesn't support 'internal' category
        let categories = if chain == "base" {
            vec![
                "external".to_string(),
                "erc20".to_string(),
                "erc721".to_string(),
                "erc1155".to_string(),
            ]
        } else {
            vec![
                "external".to_string(),
                "internal".to_string(),
                "erc20".to_string(),
                "erc721".to_string(),
                "erc1155".to_string(),
            ]
        };

        let mut all_transfers = Vec::new();
        let mut page_key: Option<String> = None;
        let mut page_count = 0;
        let max_pages = 10; // Limit to 10,000 transactions (10 pages × 1000)

        loop {
            page_count += 1;
            log::info!("Fetching {} page {}", direction, page_count);

            let params = AlchemyParams {
                from_block: from_block.to_string(),
                to_block: to_block.to_string(),
                category: categories.clone(),
                with_metadata: true,
                exclude_zero_value: false,
                max_count: "0x3e8".to_string(), // 1000 results
                from_address: from_address.map(|s| s.to_string()),
                to_address: to_address.map(|s| s.to_string()),
                page_key: page_key.clone(),
            };

            let request = AlchemyRequest {
                jsonrpc: "2.0".to_string(),
                method: "alchemy_getAssetTransfers".to_string(),
                params: vec![params],
                id: 1,
            };

            let response = self
                .client
                .post(&alchemy_url)
                .json(&request)
                .send()
                .await?;

            if !response.status().is_success() {
                return Err(format!("HTTP {}: {}", response.status(), response.text().await?).into());
            }

            let alchemy_response: AlchemyResponse = response.json().await?;

            if let Some(error) = alchemy_response.error {
                return Err(format!("Alchemy API error: {}", error.message).into());
            }

            if let Some(result) = alchemy_response.result {
                all_transfers.extend(result.transfers);
                page_key = result.page_key;

                // Stop if no more pages or reached max
                if page_key.is_none() || page_count >= max_pages {
                    break;
                }
            } else {
                break;
            }
        }

        log::info!("Fetched {} {} transfers", all_transfers.len(), direction);
        Ok(all_transfers)
    }

    /// Categorize transfer type
    fn categorize_transfer(category: &str) -> String {
        match category {
            "external" => "Transfer".to_string(),
            "internal" => "Internal Transfer".to_string(),
            "erc20" => "ERC20 Transfer".to_string(),
            "erc721" => "NFT Transfer".to_string(),
            "erc1155" => "Multi-Token Transfer".to_string(),
            _ => category.to_string(),
        }
    }

    /// Check if Alchemy API is available for a chain
    pub fn is_supported(chain: &str) -> bool {
        matches!(
            chain.to_lowercase().as_str(),
            "base" | "ethereum" | "eth" | "arbitrum" | "arb" | "optimism" | "opt" | "polygon" | "matic"
        )
    }

    /// Get Alchemy chain identifier
    pub fn get_alchemy_chain_id(chain: &str) -> String {
        match chain.to_lowercase().as_str() {
            "base" => "base".to_string(),
            "ethereum" | "eth" => "eth".to_string(),
            "arbitrum" | "arb" => "arb".to_string(),
            "optimism" | "opt" => "opt".to_string(),
            "polygon" | "matic" => "polygon".to_string(),
            _ => chain.to_lowercase(),
        }
    }
}
