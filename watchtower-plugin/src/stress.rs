use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use teos_common::appointment::Locator;
use watchtower_plugin::net::http;

use teos_common::cryptography::{get_random_keypair, sign};
use teos_common::net::http::Endpoint;
use teos_common::net::NetAddr;
use teos_common::protos as common_msgs;
use teos_common::test_utils::*;
use teos_common::UserId;

use bitcoin::secp256k1::PublicKey;

fn get_users_and_appointments_count() -> (u32, u32) {
    let n_users = std::env::args().nth(1).unwrap().parse().unwrap();
    let n_apps = std::env::args().nth(2).unwrap().parse().unwrap();
    (n_users, n_apps)
}

async fn ping(tower_net_addr: &NetAddr) -> bool {
    http::get_request(tower_net_addr, Endpoint::Ping, &None)
        .await
        .ok()
        .map_or(false, |s| s.status().is_success())
}

async fn get_appointment(
    tower_net_addr: &NetAddr,
    locator: &Locator,
    signature: String,
) -> Result<(), http::RequestError> {
    http::process_post_response::<http::ApiResponse<common_msgs::GetAppointmentResponse>>(
        http::post_request(
            tower_net_addr,
            Endpoint::GetAppointment,
            &common_msgs::GetAppointmentRequest {
                locator: locator.to_vec(),
                signature,
            },
            &None,
        )
        .await,
    )
    .await
    .map(|_| ())
}

#[tokio::main]
async fn main() {
    let tower_net_addr = NetAddr::new(String::from("http://192.168.100.6:9512"));
    let tower_id = UserId(
        PublicKey::from_str("02399c77aa0a15abd32f948c7b59ceda945586c419368c7f2a514c6d60a7354e65")
            .unwrap(),
    );

    let (n_users, n_apps) = get_users_and_appointments_count();

    // Wait for the tower to come online.
    while !ping(&tower_net_addr).await {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        println!("Waiting for the tower to come online.")
    }

    // Send appointments.
    let start = tokio::time::Instant::now();
    let mut tasks = Vec::new();
    let appointments_sent = Arc::new(Mutex::new(HashMap::new()));
    let appointment_send_times = Arc::new(Mutex::new(Vec::new()));
    for i in 0..n_users {
        let tower_net_addr = tower_net_addr.clone();
        let (sk, pk) = get_random_keypair();
        let user_id = UserId(pk);

        // Shared data.
        let appointments_sent = appointments_sent.clone();
        let appointment_send_times = appointment_send_times.clone();

        tasks.push(tokio::task::spawn(async move {
            let mut times = Vec::new();
            let mut locators = Vec::new();
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
                // Generate and send an appointment.
                let appointment = generate_random_appointment(None);
                let signature = sign(&appointment.to_vec(), &sk).unwrap();
                let start = tokio::time::Instant::now();
                http::add_appointment(tower_id, &tower_net_addr, &None, &appointment, &signature)
                    .await
                    .map_err(|e| println!("User {i} faced an error sending appointment {j}: {e:?}"))
                    .ok();
                // Store the time it took us to successfully send the appointment and the appointment locator.
                times.push(tokio::time::Instant::now() - start);
                locators.push(appointment.locator);
            }
            appointments_sent.lock().unwrap().insert(sk, (i, locators));
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
    for (sk, (i, locators)) in appointments_sent {
        let tower_net_addr = tower_net_addr.clone();

        // Shared data.
        let appointment_send_times = appointment_send_times.clone();

        tasks.push(tokio::task::spawn(async move {
            let mut times = Vec::new();
            for (j, locator) in locators.iter().enumerate() {
                let signature = sign(format!("get appointment {locator}").as_bytes(), &sk).unwrap();
                let start = tokio::time::Instant::now();
                get_appointment(&tower_net_addr, locator, signature)
                    .await
                    .map_err(|e| {
                        println!("User {i} faced an error while retrieve appointment {j}: {e:?}")
                    })
                    .ok();
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
