use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::ops::{Deref, DerefMut};
use std::str::FromStr;
use std::sync::{Arc, Condvar, Mutex};

use teos::async_listener::AsyncBlockListener;
use teos::bitcoin_cli::BitcoindClient;
use teos::carrier::Carrier;
use teos::chain_monitor::ChainMonitor;
use teos::config::{self, Config, Opt};
use teos::dbm::DBM;
use teos::gatekeeper::Gatekeeper;
use teos::responder::Responder;
use teos::watcher::Watcher;

use teos_common::constants::IRREVOCABLY_RESOLVED;
use teos_common::cryptography::{get_random_keypair, sign};
use teos_common::test_utils::*;
use teos_common::TowerId;
use teos_common::UserId;

use log::LevelFilter;
use simple_logger::SimpleLogger;
use structopt::StructOpt;

use bitcoin::network::constants::Network;
use bitcoin::secp256k1::{PublicKey, Secp256k1, SecretKey};

use bitcoincore_rpc::{Auth, Client, RpcApi};

use lightning_block_sync::init::validate_best_block_header;
use lightning_block_sync::poll::{
    ChainPoller, Poll, Validate, ValidatedBlock, ValidatedBlockHeader,
};
use lightning_block_sync::{BlockSource, BlockSourceError, SpvClient, UnboundedCache};

async fn get_last_n_blocks<B, T>(
    poller: &mut ChainPoller<B, T>,
    mut last_known_block: ValidatedBlockHeader,
    n: usize,
) -> Result<Vec<ValidatedBlock>, BlockSourceError>
where
    B: DerefMut<Target = T> + Sized + Send + Sync,
    T: BlockSource,
{
    let mut last_n_blocks = Vec::with_capacity(n);
    for _ in 0..n {
        log::debug!("Fetching block #{}", last_known_block.height);
        let block = poller.fetch_block(&last_known_block).await?;
        last_known_block = poller.look_up_previous_header(&last_known_block).await?;
        last_n_blocks.push(block);
    }

    Ok(last_n_blocks)
}

async fn create_new_tower_keypair(dbm: &DBM) -> (SecretKey, PublicKey) {
    let sk =
        SecretKey::from_str("646133513d0d57bb01269ff517b46fb4c65137741c03e535d80a606116155dd0")
            .unwrap();
    let pk = PublicKey::from_secret_key(&Secp256k1::new(), &sk);
    dbm.store_tower_key(&sk).await.unwrap();
    (sk, pk)
}

async fn stress(watcher: Arc<Watcher>) {
    let (n_users, n_apps) = (10, 100);

    // Send appointments.
    let start = tokio::time::Instant::now();
    let mut tasks = Vec::new();
    let appointments_sent = Arc::new(Mutex::new(HashMap::new()));
    let appointment_send_times = Arc::new(Mutex::new(Vec::new()));
    for _ in 0..n_users {
        let (sk, pk) = get_random_keypair();
        let user_id = UserId(pk);

        // Shared data.
        let appointments_sent = appointments_sent.clone();
        let appointment_send_times = appointment_send_times.clone();
        let watcher = watcher.clone();

        tasks.push(tokio::task::spawn(async move {
            let mut times = Vec::new();
            let mut locators = Vec::new();
            for j in 0..n_apps {
                // Re-register every once in a while.
                if j % 300 == 0 {
                    watcher.register(user_id).await.unwrap();
                }
                // Generate and send an appointment.
                let appointment = generate_random_appointment(None);
                let signature = sign(&appointment.to_vec(), &sk).unwrap();
                let start = tokio::time::Instant::now();
                locators.push(appointment.locator);
                watcher
                    .add_appointment(appointment, signature)
                    .await
                    .unwrap();
                // Store the time it took us to successfully send the appointment and the appointment locator.
                times.push(tokio::time::Instant::now() - start);
            }
            appointments_sent.lock().unwrap().insert(sk, locators);
            appointment_send_times.lock().unwrap().extend(times);
        }));
    }

    // Wait for all the tasks to finish.
    while !tasks.iter().all(|t| t.is_finished()) {
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    // Log the results.
    let appointment_send_times = Arc::try_unwrap(appointment_send_times)
        .unwrap()
        .into_inner()
        .unwrap();
    let appointments_sent = Arc::try_unwrap(appointments_sent)
        .unwrap()
        .into_inner()
        .unwrap();
    println!(
        "Took {:?} to send {} appointments.",
        tokio::time::Instant::now() - start,
        n_users * n_apps
    );
    println!(
        "Min = {:?}, Max = {:?}, Avg = {:?}",
        appointment_send_times.iter().min().unwrap(),
        appointment_send_times.iter().max().unwrap(),
        appointment_send_times.iter().sum::<tokio::time::Duration>()
            / appointment_send_times.len() as u32
    );

    // Now retrieve all these sent appointment.
    let start = tokio::time::Instant::now();
    let mut tasks = Vec::new();
    let appointment_send_times = Arc::new(Mutex::new(Vec::new()));
    for (sk, locators) in appointments_sent {
        // Shared data.
        let appointment_send_times = appointment_send_times.clone();
        let watcher = watcher.clone();

        tasks.push(tokio::task::spawn(async move {
            let mut times = Vec::new();
            for locator in locators {
                let signature = sign(format!("get appointment {locator}").as_bytes(), &sk).unwrap();
                let start = tokio::time::Instant::now();
                watcher.get_appointment(locator, &signature).await.unwrap();
                times.push(tokio::time::Instant::now() - start);
            }
            appointment_send_times.lock().unwrap().extend(times);
        }));
    }

    // Wait for all the tasks to finish.
    while !tasks.iter().all(|t| t.is_finished()) {
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    // Log the results.
    let appointment_send_times = Arc::try_unwrap(appointment_send_times)
        .unwrap()
        .into_inner()
        .unwrap();
    println!(
        "Took {:?} to retrieve {} appointments.",
        tokio::time::Instant::now() - start,
        n_users * n_apps
    );
    println!(
        "Min = {:?}, Max = {:?}, Avg = {:?}",
        appointment_send_times.iter().min().unwrap(),
        appointment_send_times.iter().max().unwrap(),
        appointment_send_times.iter().sum::<tokio::time::Duration>()
            / appointment_send_times.len() as u32
    );
}

#[tokio::main]
async fn main() {
    let opt = Opt::from_args();
    let path = config::data_dir_absolute_path(opt.data_dir.clone());
    let conf_file_path = path.join("teos.toml");
    // Create data dir if it does not exist
    fs::create_dir_all(&path).unwrap_or_else(|e| {
        eprintln!("Cannot create data dir: {e:?}");
        std::process::exit(1);
    });

    // Load conf (from file or defaults) and patch it with the command line parameters received (if any)
    let mut conf = config::from_file::<Config>(&conf_file_path);
    let is_default = conf.is_default();
    conf.patch_with_options(opt);
    conf.verify().unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });

    // Set log level
    SimpleLogger::new()
        .with_level(if conf.deps_debug {
            LevelFilter::Debug
        } else {
            LevelFilter::Warn
        })
        .with_module_level(
            "teos",
            if conf.debug {
                LevelFilter::Debug
            } else {
                LevelFilter::Info
            },
        )
        .init()
        .unwrap();

    // Create network dir
    let path_network = path.join(conf.btc_network.clone());
    fs::create_dir_all(&path_network).unwrap_or_else(|e| {
        eprintln!("Cannot create network dir: {e:?}");
        std::process::exit(1);
    });

    // Log default data dir
    log::info!("Default data directory: {:?}", &path);

    // Log datadir path
    log::info!("Using data directory: {:?}", &path_network);

    // Log config file path based on whether the config file is found or not
    if is_default {
        log::info!("Config file: {:?} (not found, skipping)", &conf_file_path);
    } else {
        log::info!("Config file: {:?}", &conf_file_path);
        conf.log_non_default_options();
    }

    let dbm = if conf.database_url == "managed" {
        DBM::new(&format!(
            "sqlite://{}",
            path_network
                // rwc = Read + Write + Create (creates the database file if not found)
                .join("teos_db.sql3?mode=rwc")
                .to_str()
                .expect("Path to the sqlite DB contains non-UTF-8 characters.")
        ))
        .await
        .unwrap()
    } else {
        DBM::new(&conf.database_url).await.unwrap()
    };
    let dbm = Arc::new(dbm);

    // Load tower secret key or create a fresh one if none is found. If overwrite key is set, create a new
    // key straightaway
    let (tower_sk, tower_pk) = {
        if conf.overwrite_key {
            log::info!("Overwriting tower keys");
            create_new_tower_keypair(&dbm).await
        } else if let Some(sk) = dbm.load_tower_key().await {
            (sk, PublicKey::from_secret_key(&Secp256k1::new(), &sk))
        } else {
            log::info!("Tower keys not found. Creating a fresh set");
            create_new_tower_keypair(&dbm).await
        }
    };
    log::info!("tower_id: {tower_pk}");

    // Initialize our bitcoind client
    let (bitcoin_cli, bitcoind_reachable) = match BitcoindClient::new(
        &conf.btc_rpc_connect,
        conf.btc_rpc_port,
        &conf.btc_rpc_user,
        &conf.btc_rpc_password,
        &conf.btc_network,
    )
    .await
    {
        Ok(client) => (
            Arc::new(client),
            Arc::new((Mutex::new(true), Condvar::new())),
        ),
        Err(e) => {
            let e_msg = match e.kind() {
                ErrorKind::InvalidData => "invalid btcrpcuser or btcrpcpassword".into(),
                _ => e.to_string(),
            };
            log::error!("Failed to connect to bitcoind. Error: {e_msg}");
            std::process::exit(1);
        }
    };

    // FIXME: Temporary. We're using bitcoin_core_rpc and rust-lightning's rpc until they both get merged
    // https://github.com/rust-bitcoin/rust-bitcoincore-rpc/issues/166
    let schema = if !conf.btc_rpc_connect.starts_with("http") {
        "http://"
    } else {
        ""
    };
    let rpc = Arc::new(
        Client::new(
            &format!("{schema}{}:{}", conf.btc_rpc_connect, conf.btc_rpc_port),
            Auth::UserPass(conf.btc_rpc_user.clone(), conf.btc_rpc_password.clone()),
        )
        .unwrap(),
    );
    let mut derefed = bitcoin_cli.deref();
    // Load last known block from DB if found. Poll it from Bitcoind otherwise.
    let last_known_block = dbm.load_last_known_block().await;
    let tip = if let Some(block_hash) = last_known_block {
        let mut last_known_header = derefed
            .get_header(&block_hash, None)
            .await
            .unwrap()
            .validate(block_hash)
            .unwrap();

        log::info!(
            "Last known block: {} (height: {})",
            last_known_header.header.block_hash(),
            last_known_header.height
        );

        // If we are running in pruned mode some data may be missing (if we happen to have been offline for a while)
        if let Some(prune_height) = rpc.get_blockchain_info().unwrap().prune_height {
            if last_known_header.height - IRREVOCABLY_RESOLVED + 1 < prune_height as u32 {
                log::warn!(
                    "Cannot load blocks in the range {}-{}. Chain has gone too far out of sync",
                    last_known_header.height - IRREVOCABLY_RESOLVED + 1,
                    last_known_header.height
                );
                if conf.force_update {
                    log::info!("Forcing a backend update");
                    // We want to grab the first IRREVOCABLY_RESOLVED we know about for the initial cache
                    // So we can perform transitions from there onwards.
                    let target_height = prune_height + IRREVOCABLY_RESOLVED as u64;
                    let target_hash = rpc.get_block_hash(target_height).unwrap();
                    last_known_header = derefed
                        .get_header(
                            &rpc.get_block_hash(target_height).unwrap(),
                            Some(target_height as u32),
                        )
                        .await
                        .unwrap()
                        .validate(target_hash)
                        .unwrap();
                } else {
                    log::error!(
                        "The underlying chain has gone too far out of sync. The tower block cache cannot be initialized. Run with --forceupdate to force update. THIS WILL, POTENTIALLY, MAKE THE TOWER MISS SOME OF ITS APPOINTMENTS"
                    );
                    std::process::exit(1);
                }
            }
        }
        last_known_header
    } else {
        validate_best_block_header(&derefed).await.unwrap()
    };

    // DISCUSS: This is not really required (and only triggered in regtest). This is only in place so the caches can be
    // populated with enough blocks mainly because the size of the cache is based on the amount of blocks passed when initializing.
    // However, we could add an additional parameter to specify the size of the cache, and initialize with however may blocks we
    // could pull from the backend. Adding this functionality just for regtest seemed unnecessary though, hence the check.
    if tip.height < IRREVOCABLY_RESOLVED {
        log::error!(
            "Not enough blocks to start teosd (required: {IRREVOCABLY_RESOLVED}). Mine at least {} more",
            IRREVOCABLY_RESOLVED - tip.height
        );
        std::process::exit(1);
    }

    log::info!(
        "Current chain tip: {} (height: {})",
        tip.header.block_hash(),
        tip.height
    );

    // This is how chain poller names bitcoin networks.
    let btc_network = match conf.btc_network.as_str() {
        "main" => "bitcoin",
        "test" => "testnet",
        any => any,
    };

    // Build components
    let gatekeeper = Arc::new(
        Gatekeeper::new(
            tip.height,
            conf.subscription_slots,
            conf.subscription_duration,
            conf.expiry_delta,
            dbm.clone(),
        )
        .await,
    );

    let mut poller = ChainPoller::new(&mut derefed, Network::from_str(btc_network).unwrap());
    let (responder, watcher) = {
        let last_n_blocks = get_last_n_blocks(&mut poller, tip, IRREVOCABLY_RESOLVED as usize)
            .await.unwrap_or_else(|e| {
                // I'm pretty sure this can only happen if we are pulling blocks from the target to the prune height, and by the time we get to
                // the end at least one has been pruned.
                log::error!("Couldn't load the latest {IRREVOCABLY_RESOLVED} blocks. Please try again (Error: {})", e.into_inner());
                std::process::exit(1);
            }
        );

        let responder = Arc::new(Responder::new(
            &last_n_blocks,
            tip.height,
            Carrier::new(rpc, bitcoind_reachable.clone(), tip.height),
            gatekeeper.clone(),
            dbm.clone(),
        ));
        let watcher = Arc::new(Watcher::new(
            gatekeeper.clone(),
            responder.clone(),
            &last_n_blocks[0..6],
            tip.height,
            tower_sk,
            TowerId(tower_pk),
            dbm.clone(),
        ));
        (responder, watcher)
    };

    if watcher.is_fresh().await & responder.is_fresh().await & gatekeeper.is_fresh().await {
        log::info!("Fresh bootstrap");
    } else {
        log::info!("Bootstrapping from backed up data");
    }

    let (_trigger, shutdown_signal_cm) = triggered::trigger();

    // The ordering here actually matters. Listeners are called by order, and we want the gatekeeper to be called
    // first so it updates the users' states and both the Watcher and the Responder operate only on registered users.
    let listeners = (gatekeeper, (watcher.clone(), responder));

    // This spawns a separate async actor in the background that will be fed new blocks from a sync block listener.
    // In this way we can have our components listen to blocks in an async manner from the async actor.
    let listener = AsyncBlockListener::wrap_listener(listeners, dbm);

    let cache = &mut UnboundedCache::new();
    let spv_client = SpvClient::new(tip, poller, cache, &listener);
    let mut chain_monitor = ChainMonitor::new(
        spv_client,
        tip,
        conf.polling_delta,
        shutdown_signal_cm,
        bitcoind_reachable.clone(),
    )
    .await;

    // Get all the components up to date if there's a backlog of blocks
    chain_monitor.poll_best_tip().await;
    log::info!("Bootstrap completed. Starting the stress test in a different thread.");

    tokio::spawn(stress(watcher));

    chain_monitor.monitor_chain().await;
}
