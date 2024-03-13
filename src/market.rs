//! This module contains structs and functions related to the openbook market.
use crate::{
    orders::{MarketInfo, OpenOrders, OpenOrdersCacheEntry},
    rpc_client::RpcClient,
    utils::{get_unix_secs, u64_slice_to_bytes},
};
use log::debug;
use openbook_dex::{
    critbit::Slab,
    instruction::SelfTradeBehavior,
    matching::{OrderType, Side},
    state::MarketState,
};
use rand::random;
use solana_client::{client_error::ClientError, rpc_request::TokenAccountsFilter};
use solana_program::{account_info::AccountInfo, pubkey::Pubkey};
use solana_rpc_client_api::config::RpcSendTransactionConfig;
use solana_sdk::sysvar::slot_history::ProgramError;
use solana_sdk::{
    account::Account,
    message::Message,
    nonce::state::Data as NonceData,
    signature::Signature,
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use spl_associated_token_account::{
    get_associated_token_address, instruction::create_associated_token_account,
};
use std::{
    cell::RefMut,
    collections::HashMap,
    error::Error,
    fmt,
    num::NonZeroU64,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

/// Struct representing a market with associated state and information.
#[derive(Debug)]
pub struct Market {
    /// The RPC client for interacting with the Solana blockchain.
    pub rpc_client: DebuggableRpcClient,

    /// The public key of the program associated with the market.
    pub program_id: Pubkey,

    /// The public key of the market.
    pub market_address: Pubkey,

    /// The keypair used for signing transactions related to the market.
    pub keypair: Keypair,

    /// The number of decimal places for the base currency (coin) in the market.
    pub coin_decimals: u8,

    /// The number of decimal places for the quote currency (pc) in the market.
    pub pc_decimals: u8,

    /// The lot size for the base currency (coin) in the market.
    pub coin_lot_size: u64,

    /// The account flags associated with the market.
    pub account_flags: u64,

    /// The lot size for the quote currency (pc) in the market.
    pub pc_lot_size: u64,

    /// The public key of the account holding USDC tokens.
    pub usdc_ata: Pubkey,

    /// The public key of the account holding WSOL tokens.
    pub wsol_ata: Pubkey,

    /// The public key of the vault holding base currency (coin) tokens.
    pub coin_vault: Pubkey,

    /// The public key of the vault holding quote currency (pc) tokens.
    pub pc_vault: Pubkey,

    /// The public key of the vault signer key associated with the market.
    pub vault_signer_key: Pubkey,

    /// The public key of the orders account associated with the market.
    pub orders_key: Pubkey,

    /// The public key of the event queue associated with the market.
    pub event_queue: Pubkey,

    /// The public key of the request queue associated with the market.
    pub request_queue: Pubkey,

    /// A HashMap containing open orders cache entries associated with their public keys.
    pub open_orders_accounts_cache: HashMap<Pubkey, OpenOrdersCacheEntry>,

    /// Information about the market.
    pub market_info: MarketInfo,
}

/// Wrapper type for RpcClient to enable Debug trait implementation.
pub struct DebuggableRpcClient(RpcClient);

/// Implement the Debug trait for the wrapper type `DebuggableRpcClient`.
impl fmt::Debug for DebuggableRpcClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Include relevant information about RpcClient
        f.debug_struct("RpcClient").finish()
    }
}

impl Market {
    /// This function is responsible for creating a new instance of the `Market` struct, representing an OpenBook market
    /// on the Solana blockchain. It requires essential parameters such as the RPC client, program ID, market address,
    /// and a keypair for transaction signing. The example demonstrates how to set up the necessary environment variables,
    /// initialize the RPC client, and read the keypair from a file path. After initializing the market, information about
    /// the newly created instance is printed for verification and further usage. Ensure that the required environment variables,
    /// such as `RPC_URL`, `KEY_PATH`, and `OPENBOOK_V1_PROGRAM_ID`, are appropriately configured before executing this method.
    ///
    /// # Arguments
    ///
    /// * `rpc_client` - The RPC client for interacting with the Solana blockchain.
    /// * `program_id` - The program ID associated with the market.
    /// * `market_address` - The public key representing the market.
    /// * `keypair` - The keypair used for signing transactions.
    ///
    /// # Returns
    ///
    /// Returns an instance of the `Market` struct with default values and configurations.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use openbook::{pubkey::Pubkey, signature::Keypair, rpc_client::RpcClient};
    /// use openbook::market::Market;
    /// use openbook::utils::read_keypair;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let rpc_url = std::env::var("RPC_URL").expect("RPC_URL is not set in .env file");
    ///     let key_path = std::env::var("KEY_PATH").expect("KEY_PATH is not set in .env file");
    ///     let market_address = std::env::var("MARKET_ID")
    ///         .expect("MARKET_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///     let program_id = std::env::var("OPENBOOK_V1_PROGRAM_ID")
    ///         .expect("OPENBOOK_V1_PROGRAM_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///    
    ///     // Create a new RPC client instance for Solana blockchain interaction.
    ///     let rpc_client = RpcClient::new(rpc_url);
    ///    
    ///     // Read the keypair for signing transactions.
    ///     let keypair = read_keypair(&key_path);
    ///    
    ///     // Initialize a new Market instance with default configurations.
    ///     let market = Market::new(rpc_client, program_id, market_address, keypair).await;
    ///    
    ///     // Print information about the initialized Market.
    ///     println!("Initialized Market: {:?}", market);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn new(
        rpc_client: RpcClient,
        program_id: Pubkey,
        market_address: Pubkey,
        keypair: Keypair,
    ) -> Self {
        let usdc_ata = Default::default();
        let wsol_ata = Default::default();
        let orders_key = Default::default();
        let coin_vault = Default::default();
        let pc_vault = Default::default();
        let vault_signer_key = Default::default();
        let event_queue = Default::default();
        let request_queue = Default::default();
        let market_info = Default::default();

        let decoded = Default::default();
        let open_orders = OpenOrders::new(market_address, decoded, keypair.pubkey());
        let mut open_orders_accounts_cache = HashMap::new();

        let open_orders_cache_entry = OpenOrdersCacheEntry {
            accounts: vec![open_orders],
            ts: 123456789,
        };

        open_orders_accounts_cache.insert(keypair.pubkey(), open_orders_cache_entry);

        let mut market = Self {
            rpc_client: DebuggableRpcClient(rpc_client),
            program_id,
            market_address,
            keypair,
            coin_decimals: 9,
            pc_decimals: 6,
            coin_lot_size: 1_000_000,
            pc_lot_size: 1,
            usdc_ata,
            wsol_ata,
            coin_vault,
            pc_vault,
            vault_signer_key,
            orders_key,
            event_queue,
            request_queue,
            account_flags: 0,
            open_orders_accounts_cache,
            market_info,
        };
        market.load().await.unwrap();
        // TODO
        // self.usdc_ata = market.get_mint_address("USDC").await.unwrap();
        // self.wsol_ata = market.get_mint_address(rpc_client, "WSOL").unwrap();
        let ata_address = market
            .find_or_create_associated_token_account(&market.keypair, &market_address)
            .await
            .unwrap();

        let usdc_ata_str = std::env::var("USDC_ATA").unwrap_or(ata_address.to_string());
        let wsol_ata_str = std::env::var("WSOL_ATA").unwrap_or(ata_address.to_string());
        let oos_key_str = std::env::var("OOS_KEY").unwrap_or(ata_address.to_string());

        market.usdc_ata = Pubkey::from_str(usdc_ata_str.as_str()).unwrap_or(ata_address);
        market.wsol_ata = Pubkey::from_str(wsol_ata_str.as_str()).unwrap_or(ata_address);
        let orders_key = Pubkey::from_str(oos_key_str.as_str());

        if orders_key.is_err() {
            debug!("Orders Key not found, creating Orders Key...");

            let key = OpenOrders::make_create_account_transaction(
                &market.rpc_client.0,
                program_id,
                &market.keypair,
                market_address,
            )
            .await
            .unwrap();
            debug!("Orders Key created successfully!");
            market.orders_key = key;
        } else {
            market.orders_key = orders_key.unwrap();
        }

        market
    }

    /// Retrieves the mint address for a given token symbol.
    ///
    /// # Arguments
    ///
    /// * `&self` - A reference to the `Market` struct.
    /// * `_token_symbol` - A string representing the token symbol.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `Pubkey` of the mint or a boxed `Error` if an error occurs.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use openbook::{pubkey::Pubkey, signature::Keypair, rpc_client::RpcClient};
    /// use openbook::market::Market;
    /// use openbook::utils::read_keypair;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let rpc_url = std::env::var("RPC_URL").expect("RPC_URL is not set in .env file");
    ///     let key_path = std::env::var("KEY_PATH").expect("KEY_PATH is not set in .env file");
    ///     let market_address = std::env::var("MARKET_ID")
    ///         .expect("MARKET_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///     let program_id = std::env::var("OPENBOOK_V1_PROGRAM_ID")
    ///         .expect("OPENBOOK_V1_PROGRAM_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///    
    ///     let rpc_client = RpcClient::new(rpc_url);
    ///    
    ///     let keypair = read_keypair(&key_path);
    ///    
    ///     let market = Market::new(rpc_client, program_id, market_address, keypair).await;
    ///    
    ///     let result = market.get_mint_address("USDC").await?;
    ///
    ///     println!("{:?}", result);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn get_mint_address(&self, _token_symbol: &str) -> Result<Pubkey, Box<dyn Error>> {
        let token_accounts = &self
            .rpc_client
            .0
            .get_token_account(&self.keypair.pubkey())
            .await?;

        for _token_account in token_accounts {
            // TODO
        }

        Err("Mint address not found".into())
    }

    /// Finds or creates an associated token account for a given wallet and mint.
    ///
    /// # Arguments
    ///
    /// * `&self` - A reference to the `Market` struct.
    /// * `wallet` - A reference to the wallet's `Keypair`.
    /// * `mint` - A reference to the mint's `Pubkey`.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `Pubkey` of the associated token account or a boxed `Error` if an error occurs.
    pub async fn find_or_create_associated_token_account(
        &self,
        wallet: &Keypair,
        mint: &Pubkey,
    ) -> Result<Pubkey, Box<dyn Error>> {
        let ata_address = get_associated_token_address(&wallet.pubkey(), mint);

        let tokens = self
            .rpc_client
            .0
            .get_token_accounts_by_owner(
                &wallet.pubkey(),
                TokenAccountsFilter::ProgramId(anchor_spl::token::ID),
            )
            .await?;

        for token in tokens {
            debug!("Found Token: {:?}", token);
            if token.pubkey == ata_address.to_string() {
                debug!("Found ATA: {:?}", ata_address);
                return Ok(ata_address);
            }
        }

        debug!("ATA not found, creating ATA");

        let create_ata_ix = create_associated_token_account(
            &wallet.pubkey(),
            &ata_address,
            mint,
            &anchor_spl::token::ID,
        );
        let message = Message::new(&[create_ata_ix], Some(&wallet.pubkey()));
        let mut transaction = Transaction::new_unsigned(message);
        let recent_blockhash = self.rpc_client.0.get_latest_blockhash().await?;
        transaction.sign(&[wallet], recent_blockhash);

        let result = self
            .rpc_client
            .0
            .send_and_confirm_transaction(&transaction)
            .await;

        match result {
            Ok(sig) => debug!("Transaction successful, signature: {:?}", sig),
            Err(err) => debug!("Transaction failed: {:?}", err),
        };

        Ok(ata_address)
    }

    /// Loads market information, including account details and state, using the provided RPC client.
    ///
    /// This function fetches and processes the necessary account information from Solana
    /// blockchain to initialize the `Market` struct. It retrieves the market state, bids
    /// information, and other relevant details.
    ///
    /// # Arguments
    ///
    /// * `&mut self` - A mutable reference to the `Market` struct.
    ///
    /// # Returns
    ///
    /// `Result` indicating success or an error if loading the market information fails.
    ///
    /// # Errors
    ///
    /// This function may return an error if there is an issue with fetching accounts
    /// or processing the market information.
    pub async fn load(&mut self) -> anyhow::Result<MarketState> {
        let mut account = self.rpc_client.0.get_account(&self.market_address).await?;
        let owner = account.owner;
        let program_id_binding = self.program_id;
        let market_account_binding = self.market_address;
        let account_info;
        {
            account_info = self.create_account_info_from_account(
                &mut account,
                &market_account_binding,
                &program_id_binding,
                false,
                false,
            );
        }
        if self.program_id != owner {
            return Err(ProgramError::InvalidArgument.into());
        }

        let market_state = self.load_market_state_bids_info(&account_info).await?;

        Ok(*market_state)
    }

    /// Loads the market state and bids information from the provided account information.
    ///
    /// # Arguments
    ///
    /// * `&mut self` - A mutable reference to the `Market` struct.
    /// * `account_info` - A reference to the account information used to load the market state.
    ///
    /// # Returns
    ///
    /// A `Result` containing a mutable reference to the loaded `MarketState` if successful,
    /// or an error if loading the market state fails.
    ///
    /// # Errors
    ///
    /// This function may return an error if there is an issue with loading the market state.
    pub async fn load_market_state_bids_info<'a>(
        &'a mut self,
        account_info: &'a AccountInfo<'_>,
    ) -> anyhow::Result<RefMut<MarketState>> {
        let account_data = account_info.deserialize_data::<NonceData>()?;
        debug!("Account Data: {:?}", account_data);
        let mut market_state = MarketState::load(account_info, &self.program_id, false)?;

        {
            let coin_vault_array: [u8; 32] = u64_slice_to_bytes(market_state.coin_vault);
            let pc_vault_array: [u8; 32] = u64_slice_to_bytes(market_state.pc_vault);
            let request_queue_array: [u8; 32] = u64_slice_to_bytes(market_state.req_q);
            let event_queue_array: [u8; 32] = u64_slice_to_bytes(market_state.event_q);

            let coin_vault_temp = Pubkey::new_from_array(coin_vault_array);
            let pc_vault_temp = Pubkey::new_from_array(pc_vault_array);
            let request_queue_temp = Pubkey::new_from_array(request_queue_array);
            let event_queue_temp = Pubkey::new_from_array(event_queue_array);

            self.coin_vault = coin_vault_temp;
            self.pc_vault = pc_vault_temp;
            self.request_queue = request_queue_temp;
            self.account_flags = market_state.account_flags;
            self.coin_lot_size = market_state.coin_lot_size;
            self.pc_lot_size = market_state.pc_lot_size;
            self.coin_lot_size = market_state.coin_lot_size;
            self.event_queue = event_queue_temp;
        }
        let _result = self.load_bids_asks_info(&mut market_state).await?;

        Ok(market_state)
    }

    /// Loads information about bids, asks, and the maximum bid price from the market state.
    ///
    /// This function fetches and processes bids information from the provided `MarketState`,
    /// including extracting the bids and asks addresses, loading the bids account, and determining
    /// the maximum bid price.
    ///
    /// # Arguments
    ///
    /// * `&self` - A reference to the `Market` struct.
    /// * `market_state` - A mutable reference to the `MarketState` representing the current state of the market.
    ///
    /// # Returns
    ///
    /// A `Result` containing a tuple of `(bids_address, asks_address, max_bid)` if successful,
    /// or an error if loading the bids information fails.
    ///
    /// # Errors
    ///
    /// This function may return an error if there is an issue with fetching accounts
    /// or processing the bids information.
    pub async fn load_bids_asks_info(
        &mut self,
        market_state: &RefMut<'_, MarketState>,
    ) -> anyhow::Result<(Pubkey, Pubkey, MarketInfo)> {
        let (bids_address, asks_address) = self.get_bids_asks_addresses(market_state);

        let mut bids_account = self.rpc_client.0.get_account(&bids_address).await?;
        let bids_info = self.create_account_info_from_account(
            &mut bids_account,
            &bids_address,
            &self.program_id,
            false,
            false,
        );
        let mut bids = market_state.load_bids_mut(&bids_info)?;
        let (open_bids, open_bids_prices, max_bid) = self.process_bids(&mut bids)?;

        let mut asks_account = self.rpc_client.0.get_account(&asks_address).await?;
        let asks_info = self.create_account_info_from_account(
            &mut asks_account,
            &asks_address,
            &self.program_id,
            false,
            false,
        );
        let mut asks = market_state.load_asks_mut(&asks_info)?;
        let (open_asks, open_asks_prices, min_ask) = self.process_asks(&mut asks)?;

        self.market_info = MarketInfo {
            min_ask,
            max_bid,
            open_asks,
            open_bids,
            bids_address,
            asks_address,
            open_asks_prices,
            open_bids_prices,
            base_total: 0.,
            quote_total: 0.,
        };

        Ok((bids_address, asks_address, self.market_info.clone()))
    }

    /// Processes bids information to find the maximum bid price.
    ///
    /// This function iteratively removes bids from the provided `Slab` until
    /// it finds the maximum bid price.
    ///
    /// # Arguments
    ///
    /// * `&self` - A reference to the `Market` struct.
    /// * `bids` - A mutable reference to the `Slab` containing bids information.
    ///
    /// # Returns
    ///
    /// A `Result` containing the maximum bid price if successful, or an error if processing bids fails.
    ///
    /// # Errors
    ///
    /// This function may return an error if there is an issue with processing the bids information.
    pub fn process_bids(
        &self,
        bids: &mut RefMut<Slab>,
    ) -> anyhow::Result<(Vec<u128>, Vec<f64>, u64)> {
        let mut max_bid = 0;
        let mut open_bids = Vec::new();
        let mut open_bids_prices = Vec::new();
        loop {
            let node = bids.remove_max();
            match node {
                Some(node) => {
                    let owner = node.owner();
                    let bytes = u64_slice_to_bytes(owner);
                    let owner_address = Pubkey::from(bytes);

                    let order_id = node.order_id();
                    let price_raw = node.price().get();
                    let ui_price = price_raw as f64 / 1e4;

                    debug!("bid: {price_raw}");

                    if max_bid == 0 {
                        max_bid = price_raw;
                    }

                    if owner_address == self.orders_key {
                        open_bids.push(order_id);
                        open_bids_prices.push(ui_price);
                    }

                    break;
                }
                None => {
                    break;
                }
            }
        }
        Ok((open_bids, open_bids_prices, max_bid))
    }

    /// Processes asks information to fetch asks info.
    ///
    /// This function iteratively removes asks from the provided `Slab` until
    /// it finds the all asks.
    ///
    /// # Arguments
    ///
    /// * `&self` - A reference to the `Market` struct.
    /// * `asks` - A mutable reference to the `Slab` containing asks information.
    ///
    /// # Returns
    ///
    /// A `Result` containing the maximum bid price if successful, or an error if processing asks fails.
    pub fn process_asks(
        &self,
        asks: &mut RefMut<Slab>,
    ) -> anyhow::Result<(Vec<u128>, Vec<f64>, u64)> {
        let mut min_ask = 0;
        let mut open_asks = Vec::new();
        let mut open_asks_prices = Vec::new();
        loop {
            let node = asks.remove_min();
            match node {
                Some(node) => {
                    let owner = node.owner();
                    let bytes = u64_slice_to_bytes(owner);
                    let owner_address = Pubkey::from(bytes);

                    let order_id = node.order_id();
                    let price_raw = node.price().get();
                    let ui_price = price_raw as f64 / 1e4;

                    debug!("ask: {price_raw}");

                    if min_ask == 0 {
                        min_ask = price_raw;
                    }

                    if owner_address == self.orders_key {
                        open_asks.push(order_id);
                        open_asks_prices.push(ui_price);
                    }
                }
                None => {
                    break;
                }
            }
        }
        Ok((open_asks, open_asks_prices, min_ask))
    }

    /// Retrieves the bids and asks addresses from the given market state.
    ///
    /// # Arguments
    ///
    /// * `&self` - A reference to the `Market` struct.
    /// * `market_state` - A reference to the `MarketState` representing the current state of the market.
    ///
    /// # Returns
    ///
    /// A tuple containing the bids and asks addresses.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use openbook::{pubkey::Pubkey, signature::Keypair, rpc_client::RpcClient};
    /// use openbook::market::Market;
    /// use openbook::utils::read_keypair;
    /// use openbook::state::MarketState;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let rpc_url = std::env::var("RPC_URL").expect("RPC_URL is not set in .env file");
    ///     let key_path = std::env::var("KEY_PATH").expect("KEY_PATH is not set in .env file");
    ///     let market_address = std::env::var("MARKET_ID")
    ///         .expect("MARKET_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///     let program_id = std::env::var("OPENBOOK_V1_PROGRAM_ID")
    ///         .expect("OPENBOOK_V1_PROGRAM_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///    
    ///     let rpc_client1 = RpcClient::new(rpc_url.clone());
    ///     let rpc_client2 = RpcClient::new(rpc_url.clone());
    ///    
    ///     let keypair = read_keypair(&key_path);
    ///    
    ///     let market = Market::new(rpc_client1, program_id, market_address, keypair).await;
    ///    
    ///     let mut account = rpc_client2.get_account(&market_address).await?;
    ///
    ///     let account_info = market.create_account_info_from_account(
    ///         &mut account,
    ///         &market_address,
    ///         &program_id,
    ///         false,
    ///         false,
    ///     );
    ///     let mut market_state = MarketState::load(&account_info, &program_id, false)?;
    ///     let (bids_address, asks_address) = market.get_bids_asks_addresses(&market_state);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn get_bids_asks_addresses(&self, market_state: &MarketState) -> (Pubkey, Pubkey) {
        let bids = market_state.bids;
        let asks = market_state.asks;
        let bids_bytes = u64_slice_to_bytes(bids);
        let asks_bytes = u64_slice_to_bytes(asks);

        let bids_address = Pubkey::new_from_array(bids_bytes);
        let asks_address = Pubkey::new_from_array(asks_bytes);

        (bids_address, asks_address)
    }

    /// Creates an `AccountInfo` instance from an `Account`.
    ///
    /// # Arguments
    ///
    /// * `&self` - A reference to the `Market` struct.
    /// * `account` - A mutable reference to the account from which to create `AccountInfo`.
    /// * `key` - A reference to the public key associated with the account.
    /// * `my_program_id` - A reference to the program's public key.
    /// * `is_signer` - A boolean indicating whether the account is a signer.
    /// * `is_writable` - A boolean indicating whether the account is writable.
    ///
    /// # Returns
    ///
    /// An `AccountInfo` instance created from the provided parameters.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use openbook::{pubkey::Pubkey, signature::Keypair, rpc_client::RpcClient};
    /// use openbook::market::Market;
    /// use openbook::utils::read_keypair;
    /// use openbook::state::MarketState;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let rpc_url = std::env::var("RPC_URL").expect("RPC_URL is not set in .env file");
    ///     let key_path = std::env::var("KEY_PATH").expect("KEY_PATH is not set in .env file");
    ///     let market_address = std::env::var("MARKET_ID")
    ///         .expect("MARKET_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///     let program_id = std::env::var("OPENBOOK_V1_PROGRAM_ID")
    ///         .expect("OPENBOOK_V1_PROGRAM_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///    
    ///     let rpc_client1 = RpcClient::new(rpc_url.clone());
    ///     let rpc_client2 = RpcClient::new(rpc_url.clone());
    ///    
    ///     let keypair = read_keypair(&key_path);
    ///    
    ///     let market = Market::new(rpc_client1, program_id, market_address, keypair).await;
    ///    
    ///     let mut account = rpc_client2.get_account(&market_address).await?;
    ///
    ///     let account_info = market.create_account_info_from_account(
    ///         &mut account,
    ///         &market_address,
    ///         &program_id,
    ///         false,
    ///         false,
    ///     );
    ///
    ///     println!("{:?}", account_info);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn create_account_info_from_account<'a>(
        &self,
        account: &'a mut Account,
        key: &'a Pubkey,
        my_program_id: &'a Pubkey,
        is_signer: bool,
        is_writable: bool,
    ) -> AccountInfo<'a> {
        AccountInfo::new(
            key,
            is_signer,
            is_writable,
            &mut account.lamports,
            &mut account.data,
            my_program_id,
            account.executable,
            account.rent_epoch,
        )
    }

    /// Places a limit bid order on the market.
    ///
    /// # Arguments
    ///
    /// * `&self` - A reference to the `Market` struct.
    /// * `max_bid` - The maximum bid value for the order.
    ///
    /// # Returns
    ///
    /// A `Result` containing the transaction signature if successful,
    /// or an error if placing the limit bid fails.
    ///
    /// # Errors
    ///
    /// This function may return an error if there is an issue with creating or sending the transaction.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use openbook::{pubkey::Pubkey, signature::Keypair, rpc_client::RpcClient};
    /// use openbook::market::Market;
    /// use openbook::utils::read_keypair;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let rpc_url = std::env::var("RPC_URL").expect("RPC_URL is not set in .env file");
    ///     let key_path = std::env::var("KEY_PATH").expect("KEY_PATH is not set in .env file");
    ///     let market_address = std::env::var("MARKET_ID")
    ///         .expect("MARKET_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///     let program_id = std::env::var("OPENBOOK_V1_PROGRAM_ID")
    ///         .expect("OPENBOOK_V1_PROGRAM_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///    
    ///     let rpc_client = RpcClient::new(rpc_url);
    ///    
    ///     let keypair = read_keypair(&key_path);
    ///    
    ///     let market = Market::new(rpc_client, program_id, market_address, keypair).await;
    ///    
    ///     let limit_bid = 2;
    ///     let result = market.place_limit_bid(limit_bid).await?;
    ///
    ///     println!("{:?}", result);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn place_limit_bid(&self, max_bid: u64) -> anyhow::Result<Signature, ClientError> {
        assert!(max_bid > 0, "Max bid must be greater than zero");
        let limit_price = NonZeroU64::new(max_bid).unwrap();
        let max_coin_qty = NonZeroU64::new(self.coin_lot_size).unwrap();
        let target_usdc_lots_w_fee = (1.0 * 1e6 * 1.1) as u64;

        let place_order_ix = openbook_dex::instruction::new_order(
            &self.market_address,
            &self.orders_key,
            &self.request_queue,
            &self.event_queue,
            &self.market_info.bids_address,
            &self.market_info.asks_address,
            &self.usdc_ata,
            &self.keypair.pubkey(),
            &self.coin_vault,
            &self.pc_vault,
            &anchor_spl::token::ID,
            &solana_program::sysvar::rent::ID,
            None,
            &self.program_id,
            Side::Bid,
            limit_price,
            max_coin_qty,
            OrderType::PostOnly,
            random::<u64>(),
            SelfTradeBehavior::AbortTransaction,
            u16::MAX,
            NonZeroU64::new(target_usdc_lots_w_fee).unwrap(),
            (get_unix_secs() + 30) as i64,
        )
        .unwrap();

        let instructions = vec![place_order_ix];

        let recent_hash = self.rpc_client.0.get_latest_blockhash().await?;
        let txn = Transaction::new_signed_with_payer(
            &instructions,
            Some(&self.keypair.pubkey()),
            &[&self.keypair],
            recent_hash,
        );

        let mut config = RpcSendTransactionConfig::default();
        config.skip_preflight = true;
        self.rpc_client
            .0
            .send_transaction_with_config(&txn, config)
            .await
    }

    /// Cancels an existing order in the market.
    ///
    /// # Arguments
    ///
    /// * `&self` - A reference to the `Market` struct.
    /// * `order_id` - The identifier of the order to be canceled.
    ///
    /// # Returns
    ///
    /// A `Result` containing the transaction signature if successful,
    /// or an error if canceling the order fails.
    ///
    /// # Errors
    ///
    /// This function may return an error if there is an issue with creating or sending the transaction.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use openbook::{pubkey::Pubkey, signature::Keypair, rpc_client::RpcClient};
    /// use openbook::market::Market;
    /// use openbook::utils::read_keypair;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let rpc_url = std::env::var("RPC_URL").expect("RPC_URL is not set in .env file");
    ///     let key_path = std::env::var("KEY_PATH").expect("KEY_PATH is not set in .env file");
    ///     let market_address = std::env::var("MARKET_ID")
    ///         .expect("MARKET_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///     let program_id = std::env::var("OPENBOOK_V1_PROGRAM_ID")
    ///         .expect("OPENBOOK_V1_PROGRAM_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///    
    ///     let rpc_client = RpcClient::new(rpc_url);
    ///    
    ///     let keypair = read_keypair(&key_path);
    ///    
    ///     let market = Market::new(rpc_client, program_id, market_address, keypair).await;
    ///    
    ///     let order_id_to_cancel = 2;
    ///     let result = market.cancel_order(order_id_to_cancel).await?;
    ///
    ///     println!("{:?}", result);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn cancel_order(&self, order_id: u64) -> anyhow::Result<Signature, ClientError> {
        assert!(order_id > 0, "Order ID must be greater than zero");

        let ix = openbook_dex::instruction::cancel_order(
            &self.program_id,
            &self.market_address,
            &self.market_info.bids_address,
            &self.market_info.asks_address,
            &self.orders_key,
            &self.keypair.pubkey(),
            &self.event_queue,
            Side::Bid,
            order_id as u128,
        )
        .unwrap();

        let instructions = vec![ix];

        let recent_hash = self.rpc_client.0.get_latest_blockhash().await?;
        let txn = Transaction::new_signed_with_payer(
            &instructions,
            Some(&self.keypair.pubkey()),
            &[&self.keypair],
            recent_hash,
        );

        let mut config = RpcSendTransactionConfig::default();
        config.skip_preflight = true;
        self.rpc_client
            .0
            .send_transaction_with_config(&txn, config)
            .await
    }

    /// Settles the balance for a user in the market.
    ///
    /// # Arguments
    ///
    /// * `&self` - A reference to the `Market` struct.
    ///
    /// # Returns
    ///
    /// A `Result` containing the transaction signature if successful,
    /// or an error if settling the balance fails.
    ///
    /// # Errors
    ///
    /// This function may return an error if there is an issue with creating or sending the transaction.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use openbook::{pubkey::Pubkey, signature::Keypair, rpc_client::RpcClient};
    /// use openbook::market::Market;
    /// use openbook::utils::read_keypair;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let rpc_url = std::env::var("RPC_URL").expect("RPC_URL is not set in .env file");
    ///     let key_path = std::env::var("KEY_PATH").expect("KEY_PATH is not set in .env file");
    ///     let market_address = std::env::var("MARKET_ID")
    ///         .expect("MARKET_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///     let program_id = std::env::var("OPENBOOK_V1_PROGRAM_ID")
    ///         .expect("OPENBOOK_V1_PROGRAM_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///    
    ///     let rpc_client = RpcClient::new(rpc_url);
    ///    
    ///     let keypair = read_keypair(&key_path);
    ///    
    ///     let market = Market::new(rpc_client, program_id, market_address, keypair).await;
    ///    
    ///     let result = market.settle_balance().await?;
    ///
    ///     println!("{:?}", result);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn settle_balance(&self) -> anyhow::Result<Signature, ClientError> {
        let ix = openbook_dex::instruction::settle_funds(
            &self.program_id,
            &self.market_address,
            &anchor_spl::token::ID,
            &self.orders_key,
            &self.keypair.pubkey(),
            &self.coin_vault,
            &self.wsol_ata,
            &self.pc_vault,
            &self.usdc_ata,
            None,
            &self.vault_signer_key,
        )
        .unwrap();

        let instructions = vec![ix];

        let recent_hash = self.rpc_client.0.get_latest_blockhash().await?;
        let txn = Transaction::new_signed_with_payer(
            &instructions,
            Some(&self.keypair.pubkey()),
            &[&self.keypair],
            recent_hash,
        );

        let mut config = RpcSendTransactionConfig::default();
        config.skip_preflight = true;
        self.rpc_client
            .0
            .send_transaction_with_config(&txn, config)
            .await
    }

    /// Creates a new transaction to match orders in the market.
    ///
    /// # Arguments
    ///
    /// * `limit` - The maximum number of orders to match.
    ///
    /// # Returns
    ///
    /// A transaction for matching orders.
    ///
    /// # Errors
    ///
    /// Returns an error if there is an issue with transaction creation or sending.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use openbook::{pubkey::Pubkey, signature::Keypair, rpc_client::RpcClient};
    /// use openbook::market::Market;
    /// use openbook::utils::read_keypair;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let rpc_url = std::env::var("RPC_URL").expect("RPC_URL is not set in .env file");
    ///     let key_path = std::env::var("KEY_PATH").expect("KEY_PATH is not set in .env file");
    ///     let market_address = std::env::var("MARKET_ID")
    ///         .expect("MARKET_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///     let program_id = std::env::var("OPENBOOK_V1_PROGRAM_ID")
    ///         .expect("OPENBOOK_V1_PROGRAM_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///    
    ///     let rpc_client = RpcClient::new(rpc_url);
    ///    
    ///     let keypair = read_keypair(&key_path);
    ///    
    ///     let market = Market::new(rpc_client, program_id, market_address, keypair).await;
    ///    
    ///     let result = market.make_match_orders_transaction(100).await?;
    ///
    ///     println!("{:?}", result);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn make_match_orders_transaction(
        &self,
        limit: u16,
    ) -> anyhow::Result<Signature, ClientError> {
        let tx = Transaction::new_with_payer(&[], Some(&self.keypair.pubkey()));

        let _match_orders_ix = openbook_dex::instruction::match_orders(
            &self.program_id,
            &self.market_address,
            &self.request_queue,
            &self.market_info.bids_address,
            &self.market_info.asks_address,
            &self.event_queue,
            &self.coin_vault,
            &self.pc_vault,
            limit,
        )
        .unwrap();

        let mut config = RpcSendTransactionConfig::default();
        config.skip_preflight = true;
        self.rpc_client
            .0
            .send_transaction_with_config(&tx, config)
            .await
    }

    /// Loads the bids from the market.
    ///
    /// # Returns
    ///
    /// The bids stored in the market.
    ///
    /// # Errors
    ///
    /// Returns an error if there is an issue with loading the bids.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use openbook::{pubkey::Pubkey, signature::Keypair, rpc_client::RpcClient};
    /// use openbook::market::Market;
    /// use openbook::utils::read_keypair;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let rpc_url = std::env::var("RPC_URL").expect("RPC_URL is not set in .env file");
    ///     let key_path = std::env::var("KEY_PATH").expect("KEY_PATH is not set in .env file");
    ///     let market_address = std::env::var("MARKET_ID")
    ///         .expect("MARKET_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///     let program_id = std::env::var("OPENBOOK_V1_PROGRAM_ID")
    ///         .expect("OPENBOOK_V1_PROGRAM_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///    
    ///     let rpc_client = RpcClient::new(rpc_url);
    ///    
    ///     let keypair = read_keypair(&key_path);
    ///    
    ///     let mut market = Market::new(rpc_client, program_id, market_address, keypair).await;
    ///    
    ///     let result = market.load_bids()?;
    ///
    ///     println!("{:?}", result);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn load_bids(&mut self) -> Result<MarketInfo, ProgramError> {
        self.load_orders(self.market_info.bids_address)
    }

    /// Loads the asks from the market.
    ///
    /// # Returns
    ///
    /// The asks stored in the market.
    ///
    /// # Errors
    ///
    /// Returns an error if there is an issue with loading the asks.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use openbook::{pubkey::Pubkey, signature::Keypair, rpc_client::RpcClient};
    /// use openbook::market::Market;
    /// use openbook::utils::read_keypair;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let rpc_url = std::env::var("RPC_URL").expect("RPC_URL is not set in .env file");
    ///     let key_path = std::env::var("KEY_PATH").expect("KEY_PATH is not set in .env file");
    ///     let market_address = std::env::var("MARKET_ID")
    ///         .expect("MARKET_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///     let program_id = std::env::var("OPENBOOK_V1_PROGRAM_ID")
    ///         .expect("OPENBOOK_V1_PROGRAM_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///    
    ///     let rpc_client = RpcClient::new(rpc_url);
    ///    
    ///     let keypair = read_keypair(&key_path);
    ///    
    ///     let mut market = Market::new(rpc_client, program_id, market_address, keypair).await;
    ///    
    ///     let result = market.load_asks()?;
    ///
    ///     println!("{:?}", result);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn load_asks(&mut self) -> Result<MarketInfo, ProgramError> {
        self.load_orders(self.market_info.asks_address)
    }

    /// Loads orders from the specified address.
    ///
    /// # Arguments
    ///
    /// * `address` - The address from which to load orders.
    ///
    /// # Returns
    ///
    /// The orders stored at the specified address.
    ///
    /// # Errors
    ///
    /// Returns an error if there is an issue with loading the orders.
    pub fn load_orders(&mut self, _address: Pubkey) -> anyhow::Result<MarketInfo, ProgramError> {
        // let account_info: Vec<u8> = self.rpc_client.0.get_account_data(&address).unwrap();
        Ok(self.market_info.clone())
    }

    /// Consumes events from the market for specified open orders accounts.
    ///
    /// # Arguments
    ///
    /// * `&self` - A reference to the `Market` struct.
    /// * `open_orders_accounts` - A vector of `Pubkey` representing the open orders accounts.
    /// * `limit` - The maximum number of events to consume.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `Signature` of the transaction or a `ClientError` if an error occurs.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use openbook::{pubkey::Pubkey, signature::Keypair, rpc_client::RpcClient};
    /// use openbook::market::Market;
    /// use openbook::utils::read_keypair;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let rpc_url = std::env::var("RPC_URL").expect("RPC_URL is not set in .env file");
    ///     let key_path = std::env::var("KEY_PATH").expect("KEY_PATH is not set in .env file");
    ///     let market_address = std::env::var("MARKET_ID")
    ///         .expect("MARKET_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///     let program_id = std::env::var("OPENBOOK_V1_PROGRAM_ID")
    ///         .expect("OPENBOOK_V1_PROGRAM_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///    
    ///     let rpc_client = RpcClient::new(rpc_url);
    ///    
    ///     let keypair = read_keypair(&key_path);
    ///    
    ///     let market = Market::new(rpc_client, program_id, market_address, keypair).await;
    ///    
    ///     let open_orders_accounts = vec![Pubkey::new_from_array([0; 32])];
    ///     let limit = 10;
    ///     let result = market.make_consume_events_instruction(open_orders_accounts, limit).await?;
    ///
    ///     println!("{:?}", result);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn make_consume_events_instruction(
        &self,
        open_orders_accounts: Vec<Pubkey>,
        limit: u16,
    ) -> Result<Signature, ClientError> {
        let consume_events_ix = openbook_dex::instruction::consume_events(
            &self.program_id,
            open_orders_accounts.iter().collect(),
            &self.market_address,
            &self.event_queue,
            &self.coin_vault,
            &self.pc_vault,
            limit,
        )
        .unwrap();

        let tx =
            Transaction::new_with_payer(&[consume_events_ix.clone()], Some(&self.keypair.pubkey()));

        let mut config = RpcSendTransactionConfig::default();
        config.skip_preflight = true;
        self.rpc_client
            .0
            .send_transaction_with_config(&tx, config)
            .await
    }

    /// Consumes permissioned events from the market for specified open orders accounts.
    ///
    /// # Arguments
    ///
    /// * `&self` - A reference to the `Market` struct.
    /// * `open_orders_accounts` - A vector of `Pubkey` representing the open orders accounts.
    /// * `limit` - The maximum number of events to consume.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `Signature` of the transaction or a `ClientError` if an error occurs.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use openbook::{pubkey::Pubkey, signature::Keypair, rpc_client::RpcClient};
    /// use openbook::market::Market;
    /// use openbook::utils::read_keypair;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let rpc_url = std::env::var("RPC_URL").expect("RPC_URL is not set in .env file");
    ///     let key_path = std::env::var("KEY_PATH").expect("KEY_PATH is not set in .env file");
    ///     let market_address = std::env::var("MARKET_ID")
    ///         .expect("MARKET_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///     let program_id = std::env::var("OPENBOOK_V1_PROGRAM_ID")
    ///         .expect("OPENBOOK_V1_PROGRAM_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///    
    ///     let rpc_client = RpcClient::new(rpc_url);
    ///    
    ///     let keypair = read_keypair(&key_path);
    ///    
    ///     let market = Market::new(rpc_client, program_id, market_address, keypair).await;
    ///    
    ///     let open_orders_accounts = vec![Pubkey::new_from_array([0; 32])];
    ///     let limit = 10;
    ///     let result = market.make_consume_events_permissioned_instruction(open_orders_accounts, limit).await?;
    ///
    ///     println!("{:?}", result);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn make_consume_events_permissioned_instruction(
        &self,
        open_orders_accounts: Vec<Pubkey>,
        limit: u16,
    ) -> Result<Signature, ClientError> {
        let consume_events_permissioned_ix =
            openbook_dex::instruction::consume_events_permissioned(
                &self.program_id,
                open_orders_accounts.iter().collect(),
                &self.market_address,
                &self.event_queue,
                &self.event_queue, // TODO: Update to consume_events_authority
                limit,
            )
            .unwrap();

        let tx = Transaction::new_with_payer(
            &[consume_events_permissioned_ix.clone()],
            Some(&self.keypair.pubkey()),
        );

        let mut config = RpcSendTransactionConfig::default();
        config.skip_preflight = true;
        self.rpc_client
            .0
            .send_transaction_with_config(&tx, config)
            .await
    }

    /// Loads open orders accounts for the owner, filtering them based on bids and asks.
    ///
    /// # Arguments
    ///
    /// * `&mut self` - A mutable reference to the `Market` struct.
    /// * `cache_duration_ms` - The duration in milliseconds for which to cache open orders accounts.
    ///
    /// # Returns
    ///
    /// A `Result` containing a vector of `Account` representing open orders accounts or a boxed `Error` if an error occurs.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use openbook::{pubkey::Pubkey, signature::Keypair, rpc_client::RpcClient};
    /// use openbook::market::Market;
    /// use openbook::utils::read_keypair;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let rpc_url = std::env::var("RPC_URL").expect("RPC_URL is not set in .env file");
    ///     let key_path = std::env::var("KEY_PATH").expect("KEY_PATH is not set in .env file");
    ///     let market_address = std::env::var("MARKET_ID")
    ///         .expect("MARKET_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///     let program_id = std::env::var("OPENBOOK_V1_PROGRAM_ID")
    ///         .expect("OPENBOOK_V1_PROGRAM_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///    
    ///     let rpc_client = RpcClient::new(rpc_url);
    ///    
    ///     let keypair = read_keypair(&key_path);
    ///    
    ///     let mut market = Market::new(rpc_client, program_id, market_address, keypair).await;
    ///
    ///     let result = market.load_orders_for_owner(5000).await?;
    ///
    ///     println!("{:?}", result);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn load_orders_for_owner(
        &mut self,
        cache_duration_ms: u64,
    ) -> Result<Vec<Account>, Box<dyn std::error::Error>> {
        let _bids = self.load_bids()?;
        let _asks = self.load_asks()?;
        let open_orders_accounts = self
            .find_open_orders_accounts_for_owner(&self.keypair.pubkey(), cache_duration_ms)
            .await?;

        Ok(open_orders_accounts)
    }

    /// Filters open orders accounts based on bids and asks.
    ///
    /// # Arguments
    ///
    /// * `&self` - A reference to the `Market` struct.
    /// * `bids` - A `MarketInfo` struct representing bids information.
    /// * `asks` - A `MarketInfo` struct representing asks information.
    /// * `open_orders_accounts` - A vector of `OpenOrders` representing open orders accounts.
    ///
    /// # Returns
    ///
    /// A filtered vector of `OpenOrders` based on bids and asks addresses.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use openbook::{pubkey::Pubkey, signature::Keypair, rpc_client::RpcClient};
    /// use openbook::market::Market;
    /// use openbook::utils::read_keypair;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let rpc_url = std::env::var("RPC_URL").expect("RPC_URL is not set in .env file");
    ///     let key_path = std::env::var("KEY_PATH").expect("KEY_PATH is not set in .env file");
    ///     let market_address = std::env::var("MARKET_ID")
    ///         .expect("MARKET_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///     let program_id = std::env::var("OPENBOOK_V1_PROGRAM_ID")
    ///         .expect("OPENBOOK_V1_PROGRAM_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///    
    ///     let rpc_client = RpcClient::new(rpc_url);
    ///    
    ///     let keypair = read_keypair(&key_path);
    ///
    ///     let market = Market::new(rpc_client, program_id, market_address, keypair).await;
    ///
    ///     let bids = market.market_info.clone();
    ///     let asks = market.market_info.clone();
    ///
    ///     let open_orders_accounts = vec![];
    ///     let result = market.filter_for_open_orders(bids, asks, open_orders_accounts);
    ///
    ///     println!("{:?}", result);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn filter_for_open_orders(
        &self,
        bids: MarketInfo,
        asks: MarketInfo,
        open_orders_accounts: Vec<OpenOrders>,
    ) -> Vec<OpenOrders> {
        let bids_address = bids.bids_address;
        let asks_address = asks.asks_address;

        open_orders_accounts
            .into_iter()
            .filter(|open_orders| {
                open_orders.address == bids_address || open_orders.address == asks_address
            })
            .collect()
    }

    /// Finds open orders accounts for a specified owner and caches them based on the specified duration.
    ///
    /// # Arguments
    ///
    /// * `&mut self` - A mutable reference to the `Market` struct.
    /// * `owner_address` - A reference to the owner's `Pubkey`.
    /// * `cache_duration_ms` - The duration in milliseconds for which to cache open orders accounts.
    ///
    /// # Returns
    ///
    /// A `Result` containing a vector of `Account` representing open orders accounts or a boxed `Error` if an error occurs.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use openbook::{pubkey::Pubkey, signature::Keypair, rpc_client::RpcClient};
    /// use openbook::market::Market;
    /// use openbook::utils::read_keypair;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let rpc_url = std::env::var("RPC_URL").expect("RPC_URL is not set in .env file");
    ///     let key_path = std::env::var("KEY_PATH").expect("KEY_PATH is not set in .env file");
    ///     let market_address = std::env::var("MARKET_ID")
    ///         .expect("MARKET_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///     let program_id = std::env::var("OPENBOOK_V1_PROGRAM_ID")
    ///         .expect("OPENBOOK_V1_PROGRAM_ID is not set in .env file")
    ///         .parse()
    ///         .unwrap();
    ///    
    ///     let rpc_client = RpcClient::new(rpc_url);
    ///    
    ///     let keypair = read_keypair(&key_path);
    ///
    ///     let mut market = Market::new(rpc_client, program_id, market_address, keypair).await;
    ///     let owner_address = &Pubkey::new_from_array([0; 32]);
    ///
    ///     let result = market.find_open_orders_accounts_for_owner(&owner_address, 5000).await?;
    ///
    ///     println!("{:?}", result);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn find_open_orders_accounts_for_owner(
        &mut self,
        owner_address: &Pubkey,
        cache_duration_ms: u64,
    ) -> Result<Vec<Account>, Box<dyn std::error::Error>> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis();
        if let Some(cache_entry) = self.open_orders_accounts_cache.get(owner_address) {
            if now - cache_entry.ts < cache_duration_ms.into() {
                // return Ok(cache_entry.accounts.clone());
            }
        }

        let open_orders_accounts_for_owner = OpenOrders::find_for_market_and_owner(
            &self.rpc_client.0,
            self.keypair.pubkey(),
            *owner_address,
            false,
        )
        .await?;
        // self.open_orders_accounts_cache.insert(
        //     *owner_address,
        //     OpenOrdersCacheEntry {
        //         accounts: open_orders_accounts_for_owner.clone(),
        //         ts: now,
        //     },
        // );

        Ok(open_orders_accounts_for_owner)
    }
}
