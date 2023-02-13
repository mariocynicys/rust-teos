//! The Eye of Satoshi - Lightning watchtower.
//!
//! A watchtower implementation written in Rust.

use rand::{distributions::Alphanumeric, Rng};
use std::{collections::HashMap, convert::TryInto};

pub type UUID = [u8; 20];
pub type Locator = [u8; 16];
pub type BLOB = Vec<u8>;

/// An extended version of the appointment hold by the tower.
///
/// The [Appointment] is extended in terms of data, that is, it provides further information only relevant to the tower.
/// Notice [ExtendedAppointment]s are not kept in memory but persisted on disk. The [Watcher](crate::watcher::Watcher)
/// keeps [AppointmentSummary] instead.
pub struct ExtendedAppointment {
    pub locator: Locator,
    /// The encrypted blob of data to be handed to the tower.
    /// Should match an encrypted penalty transaction.
    pub encrypted_blob: BLOB,
}

impl ExtendedAppointment {
    /// Create a new [ExtendedAppointment].
    pub fn new(locator: Locator, encrypted_blob: BLOB) -> Self {
        ExtendedAppointment {
            locator,
            encrypted_blob,
        }
    }

    /// Gets the underlying appointment's locator.
    pub fn locator(&self) -> Locator {
        self.locator
    }
}

pub fn get_random_bytes(size: usize) -> Vec<u8> {
    rand::thread_rng()
        .sample_iter(Alphanumeric)
        .take(size)
        .collect()
}

pub fn load_dummy_appointments(count: u32) -> HashMap<UUID, ExtendedAppointment> {
    let mut appointments = HashMap::new();
    let mut rng = rand::thread_rng();
    let log_every = count / 10;

    let encrypted_blob: BLOB = get_random_bytes(360).try_into().unwrap();

    for i in 1..=count {
        let extended_appointment = ExtendedAppointment::new(rng.gen(), encrypted_blob.clone());

        appointments.insert(rng.gen(), extended_appointment);

        if i % log_every == 0 {
            log::debug!("Generated {}% of dummy data", i * 100 / count);
        }
    }

    appointments
}
