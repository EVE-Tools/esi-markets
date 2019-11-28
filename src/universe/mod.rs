use std::fs::OpenOptions;
use std::io::{BufReader, BufWriter, Read};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use bincode::{deserialize_from, serialize_into};
use crossbeam_channel as channel;
use flate2::bufread::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use fnv::{FnvHashMap, FnvHashSet};
use parking_lot::RwLock;
use reqwest;
use serde_json;

use super::errors::*;
use super::esi;
use super::esi::types::*;

pub const BLACKLIST_PATH: &str = "cache/blacklist";

/// Represents a shared handle for querying map data and blacklisting structures
#[derive(Clone, Debug)]
pub struct Universe(Arc<RwLock<Inner>>);

#[derive(Debug)]
struct Inner {
    esi_client: esi::Client,
    regions: FnvHashSet<RegionID>,
    structures: FnvHashMap<RegionID, FnvHashSet<LocationID>>,
    structure_blacklist: FnvHashSet<LocationID>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Structure {
    region_id: RegionID,
}

impl Universe {
    /// Create a new instance of the universe structure and spawn housekeeping thread
    pub fn new(esi_client: esi::Client) -> Result<Universe> {
        let inner = Inner { esi_client,
                            regions: FnvHashSet::default(),
                            structures: FnvHashMap::default(),
                            structure_blacklist: FnvHashSet::default() };

        let new = Universe(Arc::new(RwLock::new(inner)));

        new.load_blacklist()?;
        new.load_regions()?;
        new.load_structures()?;
        new.start_housekeeping();

        Ok(new)
    }

    /// Maintain local state in regular intervals
    fn start_housekeeping(&self) {
        // Spawn thread which schedules and runs housekeeping tasks
        let clone = self.clone();

        thread::spawn(move || {
            // Convert number to milliseconds
            let milliseconds = |milliseconds| Duration::from_millis(milliseconds);
            let daily = milliseconds(24 * 60 * 60 * 1_000);
            let hourly = milliseconds(60 * 60 * 1_000);
            let five_minutes = milliseconds(5 * 60 * 1_000);

            // Create channels
            let region_update = channel::tick(daily);
            let blacklist_reset = channel::tick(daily);
            let structure_update = channel::tick(hourly);
            let save_blacklist = channel::tick(five_minutes);

            loop {
                select! {
                    recv(region_update) => {
                        let clone = clone.clone();
                        thread::spawn(move || {
                            match clone.load_regions() {
                                Ok(_) => {}
                                Err(e) => {
                                    warn!("Loading regions from ESI failed: {}", e)
                                }
                            }
                        });
                    },
                    recv(blacklist_reset) => {
                        let clone = clone.clone();
                        thread::spawn(move || {
                            clone.clear_blacklist();
                        });
                    },
                    recv(structure_update) => {
                        let clone = clone.clone();
                        thread::spawn(move || {
                            match clone.load_structures() {
                                Ok(_) => {}
                                Err(e) => {
                                    warn!("Loading structures from 3rd party API failed: {}", e)
                                }
                            }
                        });
                    },
                    recv(save_blacklist) => {
                        let clone = clone.clone();
                        thread::spawn(move || {
                            match clone.persist_blacklist() {
                                Ok(_) => {}
                                Err(e) => {
                                    warn!("Could not save structure blacklist to disk: {}", e)
                                }
                            }
                        });
                    },
                }
            }
        });
    }

    /// Get all regions with a market
    pub fn get_market_regions(&self) -> Vec<RegionID> {
        let regions = &self.0.read().regions;

        regions.clone()
               .into_iter()
               .filter(|id| {
                   // These regions do not have a market or structures
                   *id != 10_000_004 && *id != 10_000_019
               })
               .collect::<Vec<RegionID>>()
    }

    /// Return all structures in a region which are not blacklisted
    pub fn get_public_structures_in_region(&self, region_id: RegionID) -> Option<Vec<LocationID>> {
        let locations = self.0.read();
        let all_structures = locations.structures.get(&region_id);

        all_structures?;

        Some(all_structures.unwrap()
                           .difference(&locations.structure_blacklist)
                           .cloned()
                           .collect::<Vec<LocationID>>())
    }

    /// Add a structure to the blacklist
    pub fn blacklist_structure(&self, location_id: LocationID) {
        self.0.write().structure_blacklist.insert(location_id);
    }

    /// Clear structure blacklist
    fn clear_blacklist(&self) {
        self.0.write().structure_blacklist.clear();
    }

    /// Load regions from ESI
    fn load_regions(&self) -> Result<()> {
        let mut regions: FnvHashSet<RegionID> = FnvHashSet::default();
        let regions_from_esi = self.0.read().esi_client.get_region_ids()?;

        for id in regions_from_esi {
            regions.insert(id);
        }

        self.0.write().regions = regions;

        Ok(())
    }

    /// Load structures from ESI and 3rd party API
    fn load_structures(&self) -> Result<()> {
        let structure_ids = self.0.read().esi_client.get_market_structure_ids()?;
        let structure_regions = get_structure_regions()?;
        let mut result: FnvHashMap<RegionID, FnvHashSet<LocationID>> = FnvHashMap::default();

        for id in structure_ids {
            // Check if official structure is in list from 3rd party API
            let region_option = structure_regions.get(&id);

            if let Some(region) = region_option {
                // Create empty set if needed
                if result.get(region).is_none() {
                    result.insert(*region, FnvHashSet::default());
                }

                result.get_mut(region).unwrap().insert(id);
            }
        }

        self.0.write().structures = result;

        Ok(())
    }

    /// Load blacklist from disk or create cache
    fn load_blacklist(&self) -> Result<()> {
        debug!("Loading blacklist from file...");
        let blacklist_file = OpenOptions::new().read(true)
                                               .write(true)
                                               .create(true)
                                               .open(BLACKLIST_PATH)?;
        let reader = BufReader::new(blacklist_file);
        let mut deflate_reader = DeflateDecoder::new(reader);

        let blacklist_result = deserialize_from(&mut deflate_reader);

        match blacklist_result {
            Ok(blacklist) => {
                let list: FnvHashSet<LocationID> = blacklist;
                info!("Loaded {} blacklist entries from disk.", list.len());
                self.0.write().structure_blacklist = list;
            }
            Err(error) => {
                info!("No blacklist entries loaded from disk: {:?}", error);
            }
        }

        Ok(())
    }

    /// Save blacklist to disk
    pub fn persist_blacklist(&self) -> Result<()> {
        debug!("Writing blacklist to disk...");
        let blacklist_file = OpenOptions::new().write(true)
                                               .create(true)
                                               .open(BLACKLIST_PATH)?;

        let writer = BufWriter::new(blacklist_file);
        let mut deflate_writer = DeflateEncoder::new(writer, Compression::fast());

        let res = serialize_into(&mut deflate_writer, &self.0.read().structure_blacklist);

        if let Err(error) = res {
            bail!(error);
        } else {
            debug!("Successfully wrote blacklist to disk.");
        }

        Ok(())
    }
}

fn get_structure_regions() -> Result<FnvHashMap<LocationID, RegionID>> {
    let mut resp = reqwest::Client::new().get("https://stop.hammerti.me.uk/api/structure/all")
                                         .header(reqwest::header::UserAgent::new(esi::USER_AGENT))
                                         .send()?;

    if !resp.status().is_success() {
        bail!("could not load structures from 3rd party API - non-200 HTTP status!")
    }

    let mut body = String::new();
    resp.read_to_string(&mut body)?;

    let structures: FnvHashMap<String, Structure> = serde_json::from_str(&body)?;
    let mut result: FnvHashMap<LocationID, RegionID> = FnvHashMap::default();

    for (location_id, data) in structures {
        result.insert(location_id.parse()?, data.region_id);
    }

    Ok(result)
}
