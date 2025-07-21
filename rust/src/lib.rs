use bitcoincore_rpc::bitcoin::{Address, Amount, BlockHash, Network, Txid};
use bitcoincore_rpc::bitcoincore_rpc_json::AddressType;
use bitcoincore_rpc::json::LoadWalletResult;
use bitcoincore_rpc::{Auth, Client, Error as RpcError, RpcApi};
use dotenv as env;
use std::fmt::Display;
use std::fs::File;
use std::io::Write;

const INITIAL_MINING_BLOCKS: u64 = 101;
const REQUIRED_MINER_BALANCE: f64 = 20.0;
const TRANSFER_AMOUNT: u64 = 20;

#[derive(Debug)]
struct Config {
    rpc_url: String,
    rpc_user: String,
    rpc_password: String,
}

impl Config {
    fn from_env() -> Result<Self, RpcError> {
        Ok(Self {
            rpc_user: env::var("user").map_err(|_| {
                RpcError::ReturnedError("cannot load username from env file".into())
            })?,
            rpc_password: env::var("password").map_err(|_| {
                RpcError::ReturnedError("cannot load password from env file".into())
            })?,
            rpc_url: env::var("rpc_url")
                .map_err(|_| RpcError::ReturnedError("cannot load rpc-url from env file".into()))?,
        })
    }

    fn create_client(&self, wallet: &str) -> Result<Client, RpcError> {
        Client::new(
            format!("{}/wallet/{}", self.rpc_url, wallet).as_str(),
            Auth::UserPass(self.rpc_user.clone(), self.rpc_password.clone()),
        )
    }
}

#[derive(Debug)]
struct TransactionDetails {
    txid: Txid,
    miner_input_address: Address,
    miner_input_amount: f64,
    trader_output_address: Address,
    trader_output_amount: f64,
    miner_change_address: Address,
    miner_change_amount: f64,
    fee: f64,
    block_height: u64,
    confirmation_block_hash: BlockHash,
}

impl Display for TransactionDetails {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_lines().join("\n"))
    }
}

impl TransactionDetails {
    #[allow(clippy::too_many_arguments)]
    fn new(
        txid: Txid,
        miner_input_address: Address,
        miner_input_amount: f64,
        trader_output_address: Address,
        trader_output_amount: f64,
        miner_change_address: Address,
        miner_change_amount: f64,
        fee: f64,
        block_height: u64,
        confirmation_block_hash: BlockHash,
    ) -> Self {
        Self {
            txid,
            miner_input_address,
            miner_input_amount,
            trader_output_address,
            trader_output_amount,
            miner_change_address,
            miner_change_amount,
            fee,
            block_height,
            confirmation_block_hash,
        }
    }

    /// Creates TransactionDetails from RPC clients and transaction data
    fn from_rpc(
        miner_rpc: &Client,
        trader_rpc: &Client,
        tx_id: Txid,
        miner_input_address: Address,
        trader_output_address: Address,
        confirmation_block_hash: BlockHash,
    ) -> Result<Self, RpcError> {
        let (miner_input_amount, fee) = Self::get_miner_details(miner_rpc, tx_id)?;
        let trader_output_amount = Self::get_trader_amount(trader_rpc, tx_id)?;
        let (miner_change_address, miner_change_amount) =
            Self::get_change_details(miner_rpc, tx_id, &trader_output_address)?;
        let block_height = Self::get_block_height(miner_rpc, confirmation_block_hash)?;

        Ok(Self::new(
            tx_id,
            miner_input_address,
            miner_input_amount,
            trader_output_address,
            trader_output_amount,
            miner_change_address,
            miner_change_amount,
            fee,
            block_height,
            confirmation_block_hash,
        ))
    }

    fn get_miner_details(miner_rpc: &Client, tx_id: Txid) -> Result<(f64, f64), RpcError> {
        let miner_tx = miner_rpc.get_transaction(&tx_id, None)?;
        let miner_input_amount = f64::abs(
            miner_tx
                .details
                .iter()
                .map(|detail| detail.amount.to_btc())
                .sum(),
        );
        let fee = miner_tx
            .fee
            .ok_or_else(|| RpcError::ReturnedError("No fee found".into()))?
            .to_btc();

        Ok((miner_input_amount, fee))
    }

    fn get_trader_amount(trader_rpc: &Client, tx_id: Txid) -> Result<f64, RpcError> {
        let trader_tx = trader_rpc.get_transaction(&tx_id, None)?;
        Ok(trader_tx
            .details
            .iter()
            .map(|detail| detail.amount.to_btc())
            .sum())
    }

    fn get_change_details(
        miner_rpc: &Client,
        tx_id: Txid,
        recipient_output_address: &Address,
    ) -> Result<(Address, f64), RpcError> {
        let raw_tx = miner_rpc.get_raw_transaction(&tx_id, None)?;

        let change_output = raw_tx
            .output
            .iter()
            .find(|output| {
                if let Ok(addr) = Address::from_script(&output.script_pubkey, Network::Regtest) {
                    addr != *recipient_output_address
                } else {
                    false
                }
            })
            .ok_or_else(|| RpcError::ReturnedError("No change output found".into()))?;

        let change_address = Address::from_script(&change_output.script_pubkey, Network::Regtest)
            .map_err(|e| RpcError::ReturnedError(e.to_string()))?;

        let change_amount = change_output.value.to_btc();

        Ok((change_address, change_amount))
    }

    fn get_block_height(miner_rpc: &Client, block_hash: BlockHash) -> Result<u64, RpcError> {
        Ok(miner_rpc.get_block_info(&block_hash)?.height as u64)
    }

    fn to_lines(&self) -> Vec<String> {
        vec![
            self.txid.to_string(),
            self.miner_input_address.to_string(),
            self.miner_input_amount.to_string(),
            self.trader_output_address.to_string(),
            self.trader_output_amount.to_string(),
            self.miner_change_address.to_string(),
            self.miner_change_amount.to_string(),
            self.fee.to_string(),
            self.block_height.to_string(),
            self.confirmation_block_hash.to_string(),
        ]
    }
}

pub fn run_rpc_scenario() -> Result<(), RpcError> {
    let config = Config::from_env()?;

    // Connect to Bitcoin Core RPC
    let miner_rpc = config.create_client("Miner")?;
    let trader_rpc = config.create_client("Trader")?;

    // Get blockchain info
    let blockchain_info = miner_rpc.get_blockchain_info()?;
    println!("Blockchain Info: {blockchain_info:?}");

    // Create/Load the wallets, named 'Miner' and 'Trader'. Have logic to optionally create/load them if they do not exist or not loaded already.
    get_wallet(&miner_rpc, "Miner")?;
    get_wallet(&trader_rpc, "Trader")?;

    // Generate spendable balances in the Miner wallet
    let miner_input_address = miner_rpc
        .get_new_address(Some("Mining Reward"), Some(AddressType::Bech32))?
        .require_network(Network::Regtest)
        .map_err(|e| RpcError::ReturnedError(e.to_string()))?;

    // generate initial blocks to obtain the funds
    miner_rpc.generate_to_address(INITIAL_MINING_BLOCKS, &miner_input_address)?;

    // miner needs at least required balance
    let mut miner_balance = miner_rpc.get_wallet_info()?.balance;
    while miner_balance.to_btc() < REQUIRED_MINER_BALANCE {
        let _block_hash = miner_rpc.generate_to_address(1, &miner_input_address)?;
        miner_balance = miner_rpc.get_wallet_info()?.balance;
    }

    // Load Trader wallet and generate a new address
    let trader_output_address = trader_rpc
        .get_new_address(Some("BTC trades"), Some(AddressType::Bech32))?
        .require_network(Network::Regtest)
        .map_err(|e| RpcError::ReturnedError(e.to_string()))?;

    // Send BTC from Miner to Trader
    let tx_id = miner_rpc.send_to_address(
        &trader_output_address,
        Amount::from_int_btc(TRANSFER_AMOUNT),
        Some("I will send you some BTC for trading!"),
        Some("my friend best trader"),
        None,
        None,
        None,
        None,
    )?;

    // Check transaction in mempool
    let mempool_entry = miner_rpc
        .get_mempool_entry(&tx_id)
        .map_err(|e| RpcError::ReturnedError(e.to_string()))?;
    println!("Mempool Entry: {mempool_entry:?}");

    // Mine 1 block to confirm the transaction
    let confirmation_block = miner_rpc.generate_to_address(1, &miner_input_address)?;

    let transaction_details = TransactionDetails::from_rpc(
        &miner_rpc,
        &trader_rpc,
        tx_id,
        miner_input_address,
        trader_output_address,
        *confirmation_block.first().unwrap(),
    )?;

    // Write the data to ../out.txt
    println!("===");
    println!("Saving result:\n{transaction_details}");
    write_to_file(&transaction_details)?;

    Ok(())
}

fn write_to_file(details: &TransactionDetails) -> Result<(), RpcError> {
    let mut file = File::create("../out.txt")?;
    for line in details.to_lines() {
        writeln!(file, "{line}")?;
    }
    Ok(())
}

fn get_wallet(rpc: &Client, wallet_name: &str) -> bitcoincore_rpc::Result<LoadWalletResult> {
    // Check if wallet exists
    let wallets = rpc.list_wallets()?;
    let wallet_exists = wallets.iter().any(|wallet| wallet == wallet_name);

    if wallet_exists {
        // Try loading the wallet
        match rpc.load_wallet(wallet_name) {
            Ok(result) => Ok(result),
            Err(e) => {
                // If error is "already loaded" (code -4), unload and retry
                if e.to_string().contains("code: -4") {
                    rpc.unload_wallet(Some(wallet_name))?;
                    rpc.load_wallet(wallet_name)
                } else {
                    Err(e)
                }
            }
        }
    } else {
        // Try creating a new wallet
        rpc.create_wallet(wallet_name, None, None, None, None)
            .map_err(|e| {
                if e.to_string().contains("code: -4") {
                    RpcError::ReturnedError("Wallet already exists but was not listed".into())
                } else {
                    e
                }
            })
    }
}
