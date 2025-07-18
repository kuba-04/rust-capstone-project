#![allow(unused)]

use bitcoin::io::ErrorKind;
use bitcoincore_rpc::bitcoin::{Address, Amount, BlockHash, Network, SignedAmount, Txid};
use bitcoincore_rpc::bitcoincore_rpc_json::{AddressType, GetTransactionResultDetailCategory};
use bitcoincore_rpc::json::{ListReceivedByAddressResult, LoadWalletResult};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use serde::ser::{SerializeSeq, SerializeStruct};
use serde::{Deserialize, Serializer};
use serde_json::json;
use std::fs::File;
use std::io::Write;
use bitcoin::hex::{Case, DisplayHex};

// Node access params
const RPC_URL: &str = "http://127.0.0.1:18443"; // Default regtest RPC port
const RPC_USER: &str = "alice";
const RPC_PASS: &str = "password";

// You can use calls not provided in RPC lib API using the generic `call` function.
// An example of using the `send` RPC call, which doesn't have exposed API.
// You can also use serde_json `Deserialize` derivation to capture the returned json result.
fn send(rpc: &Client, addr: &str) -> bitcoincore_rpc::Result<String> {
    let args = [
        json!([{addr : 100 }]), // recipient address
        json!(null),            // conf target
        json!(null),            // estimate mode
        json!(null),            // fee rate in sats/vb
        json!(null),            // Empty option object
    ];

    #[derive(Deserialize)]
    struct SendResult {
        complete: bool,
        txid: String,
    }
    let send_result = rpc.call::<SendResult>("send", &args)?;
    assert!(send_result.complete);
    Ok(send_result.txid)
}

fn main() -> bitcoincore_rpc::Result<()> {
    // Connect to Bitcoin Core RPC
    let miner_rpc = Client::new(
        format!("{RPC_URL}/wallet/Miner").as_str(),
        Auth::UserPass(RPC_USER.to_owned(), RPC_PASS.to_owned()),
    )?;

    let wallets = miner_rpc.list_wallets().unwrap();
    println!("DEBUG: Wallets: {:?}", wallets);

    let trader_rpc = Client::new(
        format!("{RPC_URL}/wallet/Trader").as_str(),
        Auth::UserPass(RPC_USER.to_owned(), RPC_PASS.to_owned()),
    )?;

    // Get blockchain info
    let blockchain_info = miner_rpc.get_blockchain_info()?;
    println!("Blockchain Info: {:?}", blockchain_info);

    // Create/Load the wallets, named 'Miner' and 'Trader'. Have logic to optionally create/load them if they do not exist or not loaded already.
    let miner_wallet = "Miner";
    let _ = get_wallet(&miner_rpc, miner_wallet).unwrap();

    // Generate spendable balances in the Miner wallet. How many blocks needs to be mined?
    let miner_input_address = miner_rpc
        .get_new_address(Some("Mining Reward"), Some(AddressType::Bech32))?
        .require_network(Network::Regtest)
        .expect("Failed to get new address for miner");

    let miner_balance = miner_rpc.get_wallet_info().expect("wallet info");
    println!("Miner balance: {}", miner_balance.balance);

    // generate 101 blocks first to obtain the funds
    miner_rpc.generate_to_address(101, &miner_input_address)?;

    let mut miner_balance = miner_rpc.get_wallet_info().expect("Miner balance").balance;
    println!("Miner Balance: {miner_balance}");
    // miner needs at least 20 BTC
    while miner_balance.to_btc() < 20.0 {
        println!("Miner: I need to mine more blocks in order to send 20 BTC to Trader..");
        print!("Mining...");
        let _block_hash = miner_rpc.generate_to_address(1, &miner_input_address)?;
        println!("Completed");
        miner_balance = miner_rpc.get_wallet_info().expect("Miner balance").balance;
        println!("Miner Balance: {miner_balance}");
    }

    // Load Trader wallet and generate a new address
    let trader_wallet = "Trader";
    let _ = get_wallet(&trader_rpc, trader_wallet).unwrap();

    let trader_output_address = trader_rpc
        .get_new_address(Some("BTC trades"), Some(AddressType::Bech32))?
        .require_network(Network::Regtest)
        .expect("Failed to get new address for trader");

    // Send 20 BTC from Miner to Trader
    let txid = miner_rpc
        .send_to_address(
            &trader_output_address,
            Amount::from_int_btc(20),
            Some("I will send you some BTC for trading!"),
            Some("best trader"),
            None,
            None,
            None,
            None,
        )
        .expect("Failed to send BTC to trader");

    // Check transaction in mempool
    let mempool_entry = miner_rpc
        .get_mempool_entry(&txid)
        .expect("mempool entry miner");
    println!("DEBUG: Miner Mempool Entry: {:?}", &mempool_entry);

    // Mine 1 block to confirm the transaction
    let confirmation_block = miner_rpc.generate_to_address(1, &miner_input_address);

    if confirmation_block.is_err() {
        eprintln!("Failed to mine block..");
        return Err(confirmation_block.unwrap_err());
    }

    // Extract all required transaction details
    println!();
    println!("====Extract all required transaction details====");
    // Transaction ID (txid)
    println!("1. Transaction ID: {}", txid);
    println!();

    let miner_tx = miner_rpc.get_transaction(&txid, None)?;
    let miner_tx_details = miner_tx.details;

    // Miner's Input Address
    let miner_address_str = miner_input_address.to_string();
    println!("2. Miner Input Address: {}", miner_address_str);

    // Miner's Input Amount (in BTC)
    // we need to aggregate all inputs into a total amount (there could be multiple inputs)
    // let miner_input_amount: f64 = miner_tx_details.iter().map(|detail| detail.amount.to_btc()).sum();
    let miner_input_amount: f64 = f64::abs(miner_tx_details.iter().map(|detail| detail.amount.to_btc()).sum());

    println!("3. Miner input amount: {}", &miner_input_amount);
    println!();

    // Trader's Output Address
    let trader_tx = trader_rpc.get_transaction(&txid, None)?;

    let trader_address_str = trader_output_address.to_string();
    println!("4. Trader Address: {:?}", trader_address_str);
    println!();

    let trader_tx_details = trader_tx.details;
    // println!(" == DEBUG Trader details: {:?}", trader_tx_details);
    let trader_output_amount: f64 = trader_tx_details.iter().map(|detail| detail.amount.to_btc()).sum();
    println!("5. Trader Output Amount: {}", trader_output_amount);

    // Miner's Change Address
    // todo below is wrong, need to get it from main tx
    // let miner_change_address = miner_rpc
    //     .get_raw_change_address(None)
    //     .expect("Failed to get raw change address")
    //     .assume_checked();

    let miner_raw_tx = miner_rpc.decode_raw_transaction(miner_tx.hex.to_hex_string(Case::Lower), Some(true))?;
    // the below is working
    // let vout_change_address = miner_raw_tx.vout.iter()
    //     .map(|v| v.script_pub_key.address.clone())
    //     .filter(|a| a.as_ref().unwrap() != &trader_output_address)
    //     .last()
    //     .expect("No UTXOs found");
    let miner_vout = miner_raw_tx.vout.iter()
        // .map(|v| v.script_pub_key.clone())
        .filter(|v| v.script_pub_key.address.as_ref().unwrap() != &trader_output_address)
        .last()
        .expect("No UTXOs found");
    let miner_change_address = miner_vout.clone().script_pub_key.address.unwrap().require_network(Network::Regtest).unwrap();
    let miner_change_amount = miner_vout.value.to_btc();

    println!("6. Miner Change Address: {:?}", &miner_change_address);

    println!("7. Miner Change Amount: {}", &miner_change_amount);

    // Transaction Fees (in BTC)
    let fee = miner_tx.fee.expect("fee miner tx").to_btc();
    println!("8. Transaction Fee: {}", &fee);

    // Block height at which the transaction is confirmed
    // we pick up the first block hash, because in mine_block function we mine only 1 block
    let confirmation_block = confirmation_block?.first().unwrap().clone();
    let block_info = miner_rpc.get_block_info(&confirmation_block)?;
    let block_height = block_info.height as u64;

    println!("9. Block height: {}", block_height);

    // Block hash at which the transaction is confirmed
    let confirmation_block_hash = confirmation_block.clone();

    // Write the data to ../out.txt in the specified format given in readme.md
    let output = OutputFile {
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
    };

    let mut file = File::create("../out.txt")?;
    for line in output.to_lines() {
        writeln!(file, "{}", line)?;
    }

    Ok(())
}

#[derive(Debug)]
struct OutputFile {
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

impl OutputFile {
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

fn mine_block(
    miner_rpc: &Client,
    miner_reward_address: &Address,
) -> bitcoincore_rpc::Result<Vec<BlockHash>> {
    print!("Mining a new block...");
    let block_hash = miner_rpc.generate_to_address(1, &miner_reward_address);
    println!("Completed! New block created: {:?}", &block_hash);
    block_hash
}

fn get_wallet(rpc: &Client, wallet_name: &str) -> Result<LoadWalletResult, bitcoin::io::Error> {
    let wallet_exists = rpc
        .list_wallets()
        .expect("Can't list wallets")
        .iter()
        .any(|wallet| wallet.eq(wallet_name));
    let wallets = rpc.list_wallets().unwrap();
    println!("DEBUG: Wallets: {:?}", wallets);
    if wallet_exists {
        // rpc.unload_wallet(Some(wallet_name)).unwrap();
        println!("Loading <{wallet_name}>");
        let mut wallet = rpc.load_wallet(wallet_name);
        if wallet.is_err() {
            let error = wallet.err().unwrap().to_string();
            if error.contains("code: -4") {
                // based on error code the wallet is already loaded, to access it, unload it fist
                rpc.unload_wallet(Some(wallet_name))
                    .expect("Failed to unload wallet");
                wallet = rpc.load_wallet(wallet_name);
            } else {
                return Err(bitcoin::io::Error::new(ErrorKind::NotFound, error));
            }
        }
        let wallet = wallet.expect("Failed to load wallet");
        println!("Wallet <{wallet_name}> loaded successfully");
        Ok(wallet)
    } else {
        // creating new wallet
        let wallet = rpc.create_wallet(wallet_name, None, None, None, None);
        if wallet.is_err() {
            if wallet.err().unwrap().to_string().contains("code: -4") {
                return Err(bitcoin::io::Error::new(
                    ErrorKind::AlreadyExists,
                    "wallet with this name already exists",
                ));
            } else {
                return Err(bitcoin::io::Error::new(
                    ErrorKind::Other,
                    "Unable to create wallet. Try again later",
                ));
            }
        }
        println!("Wallet <{wallet_name}> created successfully");
        Ok(wallet.unwrap())
    }
}
