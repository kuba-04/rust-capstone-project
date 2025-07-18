#![allow(unused)]

use bitcoin::hex::{Case, DisplayHex};
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

    let trader_rpc = Client::new(
        format!("{RPC_URL}/wallet/Trader").as_str(),
        Auth::UserPass(RPC_USER.to_owned(), RPC_PASS.to_owned()),
    )?;

    // Get blockchain info
    let blockchain_info = miner_rpc.get_blockchain_info()?;
    println!("Blockchain Info: {blockchain_info:?}");

    // Create/Load the wallets, named 'Miner' and 'Trader'. Have logic to optionally create/load them if they do not exist or not loaded already.
    let miner_wallet = "Miner";
    let trader_wallet = "Trader";
    let _ = get_wallet(&miner_rpc, miner_wallet).unwrap();
    let _ = get_wallet(&trader_rpc, trader_wallet).unwrap();

    // Generate spendable balances in the Miner wallet. How many blocks needs to be mined?
    let miner_input_address = miner_rpc
        .get_new_address(Some("Mining Reward"), Some(AddressType::Bech32))?
        .require_network(Network::Regtest)
        .expect("new miner address");

    // generate 101 blocks first to obtain the funds
    miner_rpc.generate_to_address(101, &miner_input_address)?;

    // miner needs at least 20 BTC
    let mut miner_balance = miner_rpc.get_wallet_info().expect("Miner balance").balance;
    while miner_balance.to_btc() < 20.0 {
        let _block_hash = miner_rpc.generate_to_address(1, &miner_input_address)?;
        miner_balance = miner_rpc.get_wallet_info().expect("Miner balance").balance;
    }

    // Load Trader wallet and generate a new address
    let trader_output_address = trader_rpc
        .get_new_address(Some("BTC trades"), Some(AddressType::Bech32))?
        .require_network(Network::Regtest)
        .expect("new trader address");

    // Send 20 BTC from Miner to Trader
    let tx_id = miner_rpc
        .send_to_address(
            &trader_output_address,
            Amount::from_int_btc(20),
            Some("I will send you some BTC for trading!"),
            Some("my friend best trader"),
            None,
            None,
            None,
            None,
        )
        .expect("send BTC to trader");

    // Check transaction in mempool
    let mempool_entry = miner_rpc.get_mempool_entry(&tx_id).expect("mempool entry");

    // Mine 1 block to confirm the transaction
    let confirmation_block = miner_rpc.generate_to_address(1, &miner_input_address);

    // Extract all required transaction details
    // 1. Transaction ID (txid)
    println!("1. Transaction ID: {tx_id}");

    let miner_tx = miner_rpc.get_transaction(&tx_id, None)?;
    let miner_tx_details = miner_tx.details;

    // 2. Miner's Input Address
    let miner_address_str = miner_input_address.to_string();

    // 3. Miner's Input Amount (in BTC)
    // we need to aggregate all inputs into a total amount (there could be multiple inputs)
    let miner_input_amount = f64::abs(
        miner_tx_details
            .iter()
            .map(|detail| detail.amount.to_btc())
            .sum(),
    );

    // 4. Trader's Output Address
    let trader_address_str = trader_output_address.to_string();

    // 5. Trader Output Amount
    let trader_tx = trader_rpc.get_transaction(&tx_id, None)?;
    let trader_tx_details = trader_tx.details;
    let trader_output_amount: f64 = trader_tx_details
        .iter()
        .map(|detail| detail.amount.to_btc())
        .sum();

    // 6. Miner's Change Address
    let miner_raw_tx =
        miner_rpc.decode_raw_transaction(miner_tx.hex.to_hex_string(Case::Lower), Some(true))?;
    let miner_vout = miner_raw_tx
        .vout
        .iter()
        .filter(|v| v.script_pub_key.address.as_ref().unwrap() != &trader_output_address)
        .next_back()
        .expect("miner UTXOs");
    let miner_change_address = miner_vout
        .clone()
        .script_pub_key
        .address
        .unwrap()
        .require_network(Network::Regtest)
        .unwrap();

    // 7. Miner Change Amount
    let miner_change_amount = miner_vout.value.to_btc();

    // 8. Transaction Fees (in BTC)
    let fee = miner_tx.fee.expect("fee miner tx").to_btc();

    // Block height at which the transaction is confirmed
    // Block hash at which the transaction is confirmed
    // we pick up the first block hash, because in generate_to_address() we mine 1 block
    let confirmation_block_hash = *confirmation_block?.first().unwrap();
    let block_info = miner_rpc.get_block_info(&confirmation_block_hash)?;
    let block_height = block_info.height as u64;

    // Write the data to ../out.txt in the specified format given in readme.md
    let output = OutputFile {
        txid: tx_id,
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
        writeln!(file, "{line}")?;
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

fn get_wallet(rpc: &Client, wallet_name: &str) -> Result<LoadWalletResult, bitcoin::io::Error> {
    let wallet_exists = rpc
        .list_wallets()
        .expect("wallet list")
        .iter()
        .any(|wallet| wallet.eq(wallet_name));
    let wallets = rpc.list_wallets().unwrap();
    if wallet_exists {
        let mut wallet = rpc.load_wallet(wallet_name);
        if wallet.is_err() {
            let error = wallet.err().unwrap().to_string();
            if error.contains("code: -4") {
                // based on error code the wallet is already loaded, to access it, unload it fist
                rpc.unload_wallet(Some(wallet_name)).expect("unload wallet");
                wallet = rpc.load_wallet(wallet_name);
            } else {
                return Err(bitcoin::io::Error::new(ErrorKind::NotFound, error));
            }
        }
        Ok(wallet.unwrap())
    } else {
        // creating new wallet
        let wallet = rpc.create_wallet(wallet_name, None, None, None, None);
        if wallet.is_err() {
            return if wallet.err().unwrap().to_string().contains("code: -4") {
                Err(bitcoin::io::Error::new(
                    ErrorKind::AlreadyExists,
                    "wallet with this name already exists",
                ))
            } else {
                Err(bitcoin::io::Error::new(
                    ErrorKind::Other,
                    "Unable to create wallet. Try again later",
                ))
            };
        }
        Ok(wallet.unwrap())
    }
}
