use clap::{Parser, Subcommand};
use std::{any, fs, io::ErrorKind, path::PathBuf, str::FromStr, sync::Arc};

use ethers::{
    prelude::{
        Http, LocalWallet, NonceManagerMiddleware, Provider, Signer,
        SignerMiddleware, Wallet, H160,
    },
    providers::Middleware,
};
use serde::{Deserialize, Serialize};
use state_bridge::{
    bridge::{IBridgedWorldID, IStateBridge, StateBridge},
    root::IWorldIDIdentityManager,
    StateBridgeService,
};
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Deserialize, Serialize)]
struct Config {
    rpc_url: String,
    private_key: String,
    world_id_address: H160,
    world_id_state_bridge_addresses: Vec<H160>,
    bridged_world_id_addresses: Vec<H160>,
    relaying_period: u64,

    block_confirmations: Option<usize>,
}

#[derive(Subcommand, Debug)]
#[clap(rename_all = "snake_case")]
enum Command {
    Spawn { config: PathBuf },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args.command {
        Command::Spawn { config } => match fs::read_to_string(&config) {
            Ok(contents) => {
                println!("File contents: {}", contents);

                let config: Config =
                    toml::from_str(config.to_str().unwrap()).unwrap();

                let rpc_url = config.rpc_url;

                assert_eq!(
                    config.world_id_state_bridge_addresses.len(),
                    config.bridged_world_id_addresses.len(),
                    "There needs to be an equal amount of state bridge and bridged World ID contracts"
                );

                let relaying_period: Duration =
                    Duration::from_secs(config.relaying_period);

                let block_confirmations =
                    config.block_confirmations.unwrap_or(1);

                spawn_state_bridge_service(
                    rpc_url,
                    config.private_key,
                    config.world_id_address,
                    config.world_id_state_bridge_addresses,
                    config.bridged_world_id_addresses,
                    relaying_period,
                    block_confirmations,
                )
                .await?;
            }
            Err(error) => match error.kind() {
                ErrorKind::NotFound => {
                    println!("{:?} not found.", &config);
                }
                _ => {
                    println!("An error occurred: {}", error);
                }
            },
        },
    }

    Ok(())
}

async fn spawn_state_bridge_service(
    rpc_url: String,
    private_key: String,
    world_id_address: H160,
    world_id_state_bridge_addresses: Vec<H160>,
    bridged_world_id_addresses: Vec<H160>,
    relaying_period: Duration,
    block_confirmations: usize,
) -> anyhow::Result<()> {
    let provider = Provider::<Http>::try_from(rpc_url)
        .expect("failed to initialize Http provider");

    let chain_id = provider.get_chainid().await?.as_u64();

    let wallet = private_key
        .parse::<LocalWallet>()
        .expect("couldn't instantiate wallet from private key")
        .with_chain_id(chain_id);
    let wallet_address = wallet.address();

    let middleware = SignerMiddleware::new(provider, wallet);
    let middleware = NonceManagerMiddleware::new(middleware, wallet_address);
    let middleware = Arc::new(middleware);

    let world_id_interface =
        IWorldIDIdentityManager::new(world_id_address, middleware.clone());

    let mut state_bridge_service = StateBridgeService::new(world_id_interface)
        .await
        .expect("couldn't create StateBridgeService");

    for (state_bridge_address, bridged_world_id_address) in
        world_id_state_bridge_addresses
            .iter()
            .zip(bridged_world_id_addresses.iter())
    {
        let state_bridge_interface =
            IStateBridge::new(*state_bridge_address, middleware.clone());

        let bridged_world_id_interface =
            IBridgedWorldID::new(*bridged_world_id_address, middleware.clone());

        let state_bridge = StateBridge::new(
            state_bridge_interface,
            bridged_world_id_interface,
            relaying_period,
        )
        .unwrap();

        state_bridge_service.add_state_bridge(state_bridge);
    }

    state_bridge_service
        .spawn()
        .await
        .expect("failed to spawn a state bridge service");

    Ok(())
}
