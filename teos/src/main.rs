use std::collections::{HashMap, HashSet};
use std::iter::FromIterator;

use structopt::StructOpt;

use teos::{load_dummy_appointments, Locator, UUID};

#[derive(StructOpt, Debug, Clone)]
#[structopt(rename_all = "lowercase")]
pub struct Opt {
    #[structopt(long)]
    /// Whether to use `iter()` when iterating over the dummy appointments or not.
    pub use_iter: bool,

    #[structopt(long, default_value = "2000000")]
    /// How much dummy appointments to generate.
    pub count: u32,
}

fn main() {
    let opt = Opt::from_args();
    simple_logger::init_with_level(log::Level::Debug).unwrap();

    let mut locator_uuid_map: HashMap<Locator, HashSet<UUID>> = HashMap::new();
    {
        let apps_map = load_dummy_appointments(opt.count);
        // When using `iter()`, the memory consumption goes up but then goes down after trimming.
        //
        // When not using `iter()` and directly consuming the object, the memory consumption
        // gets high and doesn't decrease as much after trimming.
        if opt.use_iter {
            // Get an iterator of and consume it. Notice that our original object isn't consumed.
            for (uuid, appointment) in apps_map.iter() {
                let uuid = *uuid;
                if let Some(map) = locator_uuid_map.get_mut(&appointment.locator()) {
                    map.insert(uuid);
                } else {
                    locator_uuid_map.insert(appointment.locator(), HashSet::from_iter(vec![uuid]));
                }
            }
        } else {
            // Consume the HashMap object directly.
            for (uuid, appointment) in apps_map {
                if let Some(map) = locator_uuid_map.get_mut(&appointment.locator()) {
                    map.insert(uuid);
                } else {
                    locator_uuid_map.insert(appointment.locator(), HashSet::from_iter(vec![uuid]));
                }
            }
        }
    }

    log::debug!("Will start trimming down memory usage now");
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3));
        unsafe {
            if libc::malloc_trim(0) == 1 {
                log::debug!("Memory released")
            } else {
                log::debug!("No memory freed")
            }
        }
    }
}
