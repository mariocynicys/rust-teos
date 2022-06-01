use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use backoff::future::retry_notify;
use backoff::{Error, ExponentialBackoff};

use teos_common::cryptography;
use teos_common::errors;
use teos_common::UserId as TowerId;

use crate::net::http::{add_appointment, AddAppointmentError};
use crate::wt_client::WTClient;
use crate::AppointmentStatus;

pub async fn manage_retry(unreachable_towers: Receiver<TowerId>, wt_client: Arc<Mutex<WTClient>>) {
    log::info!("Starting retry manager");

    loop {
        let tower_id = unreachable_towers.recv().unwrap();
        wt_client
            .lock()
            .unwrap()
            .set_tower_status(tower_id, crate::TowerStatus::TemporaryUnreachable);

        log::info!("Retrying tower {}", tower_id);
        match retry_notify(
            ExponentialBackoff::default(),
            || async { do_retry(tower_id, wt_client.clone()).await },
            |err, _| {
                log::warn!("Retry error happened with {}. {}", tower_id, err);
            },
        )
        .await
        {
            Ok(_) => {
                log::info!("Retry strategy succeeded for {}", tower_id);
                wt_client
                    .lock()
                    .unwrap()
                    .set_tower_status(tower_id, crate::TowerStatus::Reachable);
            }
            Err(e) => {
                log::warn!("Retry strategy gave up for {}. {}", tower_id, e);
                log::warn!("Setting {} as unreachable", tower_id);
                wt_client
                    .lock()
                    .unwrap()
                    .set_tower_status(tower_id, crate::TowerStatus::Unreachable);
            }
        }
    }
}

async fn do_retry(
    tower_id: TowerId,
    wt_client: Arc<Mutex<WTClient>>,
) -> Result<(), Error<&'static str>> {
    let appointments = wt_client
        .lock()
        .unwrap()
        .dbm
        .lock()
        .unwrap()
        .load_appointments(tower_id, AppointmentStatus::Pending);
    let net_addr = wt_client
        .lock()
        .unwrap()
        .towers
        .get(&tower_id)
        .unwrap()
        .net_addr
        .clone();
    let user_sk = wt_client.lock().unwrap().user_sk;

    for appointment in appointments {
        match add_appointment(
            tower_id,
            &net_addr,
            &appointment,
            &cryptography::sign(&appointment.to_vec(), &user_sk).unwrap(),
        )
        .await
        {
            Ok((slots, receipt)) => {
                let mut wt_client = wt_client.lock().unwrap();
                wt_client.add_appointment_receipt(tower_id, appointment.locator, slots, &receipt);
                wt_client.remove_pending_appointment(tower_id, appointment.locator);
                log::debug!("Response verified and data stored in the database");
            }
            Err(e) => match e {
                AddAppointmentError::RequestError(e) => {
                    if e.is_connection() {
                        log::warn!(
                            "{} cannot be reached. Tower will be retried later",
                            tower_id,
                        );
                        return Err(Error::transient("Tower cannot be reached"));
                    }
                }
                AddAppointmentError::ApiError(e) => match e.error_code {
                    errors::INVALID_SIGNATURE_OR_SUBSCRIPTION_ERROR => {
                        log::warn!("There is a subscription issue with {}", tower_id);
                        return Err(Error::transient("Subscription error"));
                    }
                    _ => {
                        log::warn!(
                            "{} rejected the appointment. Error: {}, error_code: {}",
                            tower_id,
                            e.error,
                            e.error_code
                        );
                        wt_client
                            .lock()
                            .unwrap()
                            .add_invalid_appointment(tower_id, &appointment);
                    }
                },
                AddAppointmentError::SignatureError(proof) => {
                    log::warn!("Cannot recover known tower_id from the appointment receipt. Flagging tower as misbehaving");
                    wt_client
                        .lock()
                        .unwrap()
                        .flag_misbehaving_tower(tower_id, proof);
                    return Err(Error::permanent("Tower misbehaved"));
                }
            },
        }
    }
    Ok(())
}
