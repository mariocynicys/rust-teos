use std::str::FromStr;
use std::sync::{Arc, Mutex};

use watchtower_plugin::net::http;

use teos_common::cryptography::{get_random_keypair, sign};
use teos_common::net::http::Endpoint;
use teos_common::net::NetAddr;
use teos_common::test_utils::*;
use teos_common::UserId;

use bitcoin::secp256k1::PublicKey;

async fn ping(tower_net_addr: &NetAddr) -> bool {
    http::get_request(tower_net_addr, Endpoint::Ping, &None)
        .await
        .ok()
        .map_or(false, |s| s.status().is_success())
}

#[tokio::main]
async fn main() {
    let tower_net_addr = NetAddr::new(String::from("http://localhost:9512"));
    let tower_id = UserId(
        PublicKey::from_str("02399c77aa0a15abd32f948c7b59ceda945586c419368c7f2a514c6d60a7354e65")
            .unwrap(),
    );

    while !ping(&tower_net_addr).await {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        eprintln!("Waiting for the tower to come online.")
    }

    let start = tokio::time::Instant::now();

    let n_users = 100;
    let n_apps = 100;
    let mut tasks = Vec::new();
    let app_send_times = Arc::new(Mutex::new(Vec::new()));
    for i in 0..n_users {
        let tower_net_addr = tower_net_addr.clone();
        let (sk, pk) = get_random_keypair();
        let user_id = UserId(pk);
        let app_send_times = app_send_times.clone();

        let task = tokio::task::spawn(async move {
            for j in 0..n_apps {
                // Re-register every once in a while.
                if j % 300 == 0 {
                    http::register(tower_id, user_id, &tower_net_addr, &None)
                        .await
                        .map_err(|e| {
                            println!("User {i} faced an error while registering {j}: {e:?}")
                        })
                        .ok();
                }
                let appointment = generate_random_appointment(None);
                let signature = sign(&appointment.to_vec(), &sk).unwrap();
                let start = tokio::time::Instant::now();
                http::add_appointment(tower_id, &tower_net_addr, &None, &appointment, &signature)
                    .await
                    .map_err(|e| println!("User {i} faced an error sending appointment {j}: {e:?}"))
                    .ok();
                app_send_times
                    .lock()
                    .unwrap()
                    .push(tokio::time::Instant::now() - start);
                if (j + 1) % (n_apps / 10) == 0 {
                    eprintln!("User {i} sent {}/{n_apps}.", j + 1)
                }
            }
        });
        tasks.push(task);
    }

    // Wait for all the tasks to finish.
    while !tasks.iter().all(|t| t.is_finished()) {
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }
    let duration = tokio::time::Instant::now() - start;

    println!(
        "Took {:?} to send {} appointments.",
        duration,
        n_users * n_apps
    );

    let app_send_times = app_send_times.lock().unwrap();
    println!(
        "Min = {:?}, Max = {:?}, Avg = {:?}",
        app_send_times.iter().min().unwrap(),
        app_send_times.iter().max().unwrap(),
        app_send_times.iter().sum::<tokio::time::Duration>() / app_send_times.iter().count() as u32
    );
}
