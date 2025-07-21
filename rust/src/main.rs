use rust::run_rpc_scenario;

fn main() -> bitcoincore_rpc::Result<()> {
    run_rpc_scenario().map_err(|e| bitcoincore_rpc::Error::ReturnedError(e.to_string()))
}
