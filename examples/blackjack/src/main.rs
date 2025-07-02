use clap::Parser;
use itertools::Itertools;
use kaspa_addresses::{Address, Prefix, Version};
use kaspa_consensus_core::{
    network::{NetworkId, NetworkType},
    tx::{TransactionOutpoint, UtxoEntry},
};
use kaspa_wrpc_client::prelude::*;
use log::*;
use rand::Rng;
use secp256k1::{Keypair, PublicKey, SecretKey};
use std::{
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::channel,
        Arc,
    },
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use kdapp::{
    engine::{self, EpisodeMessage},
    episode::{EpisodeEventHandler, EpisodeId},
    generator::{self, PatternType, PrefixType},
    pki::{generate_keypair, PubKey},
    proxy::{self, connect_client},
};

use blackjack_episode::{BlackjackCommand, BlackjackState, BlackjackEpisode};

pub mod blackjack_episode;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Kaspa schnorr private key
    #[arg(short, long)]
    kaspa_private_key: Option<String>,

    /// Game private key
    #[arg(short = 'g', long)]
    game_private_key: Option<String>,

    /// Game opponent public key
    #[arg(short = 'o', long)]
    game_opponent_key: Option<String>,

    /// Indicates whether to run the interaction over mainnet (default: testnet 10)
    #[arg(short, long, default_value_t = false)]
    mainnet: bool,

    /// Specifies the wRPC Kaspa Node URL to use. Usage: <wss://localhost>. Defaults to the Public Node Network (PNN).
    #[arg(short, long)]
    wrpc_url: Option<String>,

    /// Logging level for all subsystems {off, error, warn, info, debug, trace}
    ///  -- You may also specify `<subsystem>=<level>,<subsystem2>=<level>,...` to set the log level for individual subsystems
    #[arg(long = "loglevel", default_value = format!("info,{}=trace", env!("CARGO_PKG_NAME")))]
    log_level: String,
}

#[tokio::main]
async fn main() {
    // Get CLI arguments
    let args = Args::parse();

    // Init logger
    kaspa_core::log::init_logger(None, &args.log_level);

    // Select network
    let (network, prefix) = if args.mainnet {
        (NetworkId::new(NetworkType::Mainnet), Prefix::Mainnet)
    } else {
        (NetworkId::with_suffix(NetworkType::Testnet, 10), Prefix::Testnet)
    };

    // Generate or obtain Kaspa private key
    let kaspa_signer = if let Some(private_key_hex) = args.kaspa_private_key {
        let mut private_key_bytes = [0u8; 32];
        faster_hex::hex_decode(private_key_hex.as_bytes(), &mut private_key_bytes).unwrap();
        Keypair::from_seckey_slice(secp256k1::SECP256K1, &private_key_bytes).unwrap()
    } else {
        let (sk, pk) = &secp256k1::generate_keypair(&mut rand::thread_rng());
        info!(
            "Generated private key {} and address {}. Send some funds to this address and rerun with `--kaspa-private-key {}`",
            sk.display_secret(),
            String::from(&Address::new(prefix, Version::PubKey, &pk.x_only_public_key().0.serialize())),
            sk.display_secret()
        );
        return;
    };

    // Extract Kaspa address
    let kaspa_addr = Address::new(prefix, Version::PubKey, &kaspa_signer.x_only_public_key().0.serialize());

    // Obtain game keys
    let (sk, player_pk) = if let Some(game_key_hex) = args.game_private_key {
        let pair = Keypair::from_str(&game_key_hex).unwrap();
        (pair.secret_key(), PubKey(pair.public_key()))
    } else {
        let (sk, pk) = generate_keypair();
        info!("Player private key: {}", sk.display_secret());
        (sk, pk)
    };

    info!("Player public key: {}", player_pk);

    // ... and opponent pk
    let opponent_pk = args.game_opponent_key.map(|opponent_key_hex| PubKey(PublicKey::from_str(&opponent_key_hex).unwrap()));

    // Connect kaspad clients
    let kaspad = connect_client(network, args.wrpc_url.clone()).await.unwrap();
    let player_kaspad = connect_client(network, args.wrpc_url).await.unwrap();

    // Define channels and exit flag
    let (sender, receiver) = channel();
    let (response_sender, response_receiver) = tokio::sync::mpsc::unbounded_channel();
    let exit_signal = Arc::new(AtomicBool::new(false));
    let exit_signal_receiver = exit_signal.clone();

    // Run the engine
    let mut engine = engine::Engine::<BlackjackEpisode, BlackjackHandler>::new(receiver);
    let engine_task = tokio::task::spawn_blocking(move || {
        engine.start(vec![BlackjackHandler { sender: response_sender, player: player_pk }]);
    });

    // Run the player task
    let player_task = tokio::spawn(async move {
        play_blackjack(player_kaspad, kaspa_signer, kaspa_addr, response_receiver, exit_signal, sk, player_pk, opponent_pk).await;
    });

    // Run the kaspad listener
    proxy::run_listener(kaspad, std::iter::once((PREFIX, (PATTERN, sender))).collect(), exit_signal_receiver).await;

    engine_task.await.unwrap();
    player_task.await.unwrap();
}

// TODO: derive pattern from prefix (using prefix as a random seed for composing the pattern)
const PATTERN: PatternType = [(7, 0), (32, 1), (45, 0), (99, 1), (113, 0), (126, 1), (189, 0), (200, 1), (211, 0), (250, 1)];
const PREFIX: PrefixType = 858598618;
const FEE: u64 = 5000;

struct BlackjackHandler {
    sender: UnboundedSender<(EpisodeId, BlackjackState)>,
    player: PubKey, // The local player pubkey
}

impl EpisodeEventHandler<BlackjackEpisode> for BlackjackHandler {
    fn on_initialize(&self, episode_id: kdapp::episode::EpisodeId, episode: &BlackjackEpisode) {
        if episode.players.contains(&self.player) {
            let _ = self.sender.send((episode_id, episode.poll()));
        }
    }

    fn on_command(
        &self,
        episode_id: kdapp::episode::EpisodeId,
        episode: &BlackjackEpisode,
        _cmd: &<BlackjackEpisode as kdapp::episode::Episode>::Command,
        _authorization: Option<PubKey>,
        _metadata: &kdapp::episode::PayloadMetadata,
    ) {
        if episode.players.contains(&self.player) {
            let _ = self.sender.send((episode_id, episode.poll()));
        }
    }

    fn on_rollback(&self, _episode_id: kdapp::episode::EpisodeId, _episode: &BlackjackEpisode) {}
}

async fn play_blackjack(
    kaspad: KaspaRpcClient,
    kaspa_signer: Keypair,
    kaspa_addr: Address,
    mut response_receiver: UnboundedReceiver<(EpisodeId, BlackjackState)>,
    exit_signal: Arc<AtomicBool>,
    sk: SecretKey,
    player_pk: PubKey,
    opponent_pk: Option<PubKey>,
) {
    let entries = kaspad.get_utxos_by_addresses(vec![kaspa_addr.clone()]).await.unwrap();
    assert!(!entries.is_empty());
    // Try to avoid collisions if both players are using the same kaspa address
    let entry = if opponent_pk.is_some() { entries.first().cloned() } else { entries.last().cloned() };
    let mut utxo = entry.map(|entry| (TransactionOutpoint::from(entry.outpoint), UtxoEntry::from(entry.utxo_entry))).unwrap();

    let generator = generator::TransactionGenerator::new(kaspa_signer, PATTERN, PREFIX);

    // When opponent pk is passed, we are expected to initiate the game
    if let Some(opponent_pk) = opponent_pk {
        // Use a simple rand method
        // TODO: a complete implementation must handle collisions
        let episode_id = rand::thread_rng().gen();
        let new_episode = EpisodeMessage::<BlackjackEpisode>::NewEpisode { episode_id, participants: vec![player_pk, opponent_pk] };
        let tx = generator.build_command_transaction(utxo, &kaspa_addr, &new_episode, FEE);
        info!("Submitting initialize command: {}", tx.id());
        let _res = kaspad.submit_transaction(tx.as_ref().into(), false).await.unwrap();
        utxo = generator::get_first_output_utxo(&tx);
    }

    let (episode_id, mut state) = response_receiver.recv().await.unwrap();
    state.print();

    let mut input = String::new();

    loop {
        use blackjack_episode::BlackjackGameStatus;
        let cmd = match state.status {
            BlackjackGameStatus::Pending => {
                println!("Enter 'deal' to start the game.");
                read_input(&mut input)
            },
            BlackjackGameStatus::PlayerTurn => {
                println!("Your turn. Enter 'hit' or 'stand'.");
                read_input(&mut input)
            },
            _ => { // Game is over
                println!("Game over. Enter 'deal' to play again, or 'exit' to quit.");
                let command_str = read_input(&mut input);
                if command_str == "exit" {
                    exit_signal.store(true, Ordering::Relaxed);
                    break;
                }
                command_str
            }
        };

        let blackjack_cmd = match cmd.as_str() {
            "deal" => Some(BlackjackCommand::Deal),
            "hit" => Some(BlackjackCommand::Hit),
            "stand" => Some(BlackjackCommand::Stand),
            _ => {
                println!("Invalid command.");
                continue;
            }
        };

        if let Some(cmd) = blackjack_cmd {
            let step = EpisodeMessage::<BlackjackEpisode>::new_signed_command(episode_id, cmd, sk, player_pk);
            let tx = generator.build_command_transaction(utxo, &kaspa_addr, &step, FEE);
            info!("Submitting command: {}", tx.id());
            let _res = kaspad.submit_transaction(tx.as_ref().into(), false).await.unwrap();
            utxo = generator::get_first_output_utxo(&tx);

            // Wait for the state to update
            let (received_id, new_state) = response_receiver.recv().await.unwrap();
            assert_eq!(episode_id, received_id);
            state = new_state;
            state.print();
        }
    }
}

fn read_input(buffer: &mut String) -> String {
    buffer.clear();
    std::io::stdin().read_line(buffer).unwrap();
    buffer.trim().to_lowercase()
}
