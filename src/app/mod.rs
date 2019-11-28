use super::config;
use super::errors::*;
use super::esi;
use super::grpc;
use super::store;
use super::universe;

use std::process;
use std::sync::Arc;
use std::thread;
use std::time;
use std::time::Duration as StandardDuration;
use std::time::Instant;

use super::esi::types::*;
use chrono::prelude::*;
use chrono::Duration;
use crossbeam_channel as channel;
use ctrlc;
use fnv::FnvHashMap;
use rand::{thread_rng, Rng};

use super::errors;

pub fn run(config: config::Config) -> Result<()> {
    debug!("{:?}", config);

    // oAuth context, independent of client so different identities are supported
    let context = esi::OAuthContext::new(config.client_id, config.secret_key, config.refresh_token);

    // Main shared ESI client using default oAuth context
    let client = esi::Client::new(context);

    // Store instance for storing orders
    let order_store = store::Store::new()?;

    // Universe instance holding map-data and structure blacklists
    let uni = universe::Universe::new(client.clone())?;

    // Start scheduling region updates
    start_schedule_loop(client, uni.clone(), order_store.clone());

    let cloned_store = order_store.clone();

    // Gracefully handle process termination
    ctrlc::set_handler(move || {
        warn!("Received shutdown signal. Saving caches...");

        match uni.persist_blacklist() {
            Ok(_) => {}
            Err(e) => warn!("Saving blacklist to disk failed: {}", e),
        }

        match cloned_store.persist_store() {
            Ok(_) => {}
            Err(e) => warn!("Saving store to disk failed: {}", e),
        }

        warn!("Done, bye!");
        process::exit(0);
    }).expect("Error setting shutdown handler!");

    // Launch gRPC server
    grpc::run_server(order_store, &config.grpc_host);

    Ok(())
}

/// Control scraping of regions in regular intervals
fn start_schedule_loop(client: esi::Client, uni: universe::Universe, order_store: store::Store) {
    thread::spawn(move || {
        // Build initial schedule
        let regions = uni.get_market_regions();
        let mut schedule: FnvHashMap<RegionID, DateTime<Utc>> = FnvHashMap::default();
        let mut rng = thread_rng();
        for region in regions {
            let random: i64 = rng.gen_range(0, 300);
            let random_time = Utc::now() + Duration::seconds(random);
            schedule.insert(region, random_time);
        }

        // Convert number to milliseconds
        let milliseconds = |milliseconds| StandardDuration::from_millis(milliseconds);

        // Define tick intervals
        let hourly = milliseconds(60 * 60 * 1_000);
        let five_times_per_second = milliseconds(200);

        // Create channels
        let region_update = channel::tick(hourly); // hourly (backend refreshes daily)
        let run_regions = channel::tick(five_times_per_second);
        let (send_reschedule, receive_reschedule) =
            channel::unbounded::<(RegionID, DateTime<Utc>)>();

        loop {
            select! {
                recv(region_update) => {
                    let regions = uni.get_market_regions();
                    for region in regions {
                        let random: i64 = rng.gen_range(0, 300);
                        let random_time = Utc::now() + Duration::seconds(random);
                        schedule.entry(region).or_insert(random_time);
                    }
                },
                recv(run_regions) => {
                    let now = &Utc::now();
                    let mut fetch: Vec<RegionID> = Vec::new();

                    for (region, time) in &schedule {
                        if time < now {
                            fetch.push(*region);
                        }
                    }

                    for region in fetch {
                        // Run in 10 minutes unless re-scheduled by self
                        schedule.insert(region, *now + Duration::seconds(300));

                        update_region(region, client.clone(), uni.clone(), order_store.clone(), send_reschedule.clone());
                    }
                },
                recv(receive_reschedule, data) => {
                    if let Some((region_id, run_at)) = data {
                        schedule.insert(region_id, run_at);
                    }
                },
            }
        }
    });
}

/// Download and store a region's market
/// It works like this (for regions/structures):
/// Spawn thread per region -> get metadata -> spawn thread per page
fn update_region(region_id: RegionID,
                 client: esi::Client,
                 uni: universe::Universe,
                 mut order_store: store::Store,
                 reschedule_channel: channel::Sender<(RegionID, DateTime<Utc>)>)
                 -> thread::JoinHandle<Result<Arc<store::UpdateResult>>> {
    thread::spawn(move || {
        let before_download = Instant::now();
        let orders = match download_region(region_id, client, uni, reschedule_channel) {
            Ok(result) => result,
            Err(e) => {
                warn!("Could not download region {}: {}", region_id, e);
                for e in e.iter().skip(1) {
                    warn!("caused by: {}", e);
                }
                bail!(e);
            }
        };
        let after_download = Instant::now();

        let before_store = Instant::now();
        let r = order_store.store_region(region_id, orders);
        let after_store = Instant::now();

        let dd = after_download.duration_since(before_download);
        let download_time_ms = (dd.as_secs() * 1_000) + u64::from(dd.subsec_millis());
        let sd = after_store.duration_since(before_store);
        let store_time_ms = (sd.as_secs() * 1_000) + u64::from(sd.subsec_millis());
        info!("{:8} - New: {:6}\tUpdated: {:6}\tClosed: {:6}\tUnaffected: {:6}\tAffected RegionTypes: {:6}\tDownload: {:5}ms\tStore: {:5}ms", region_id, r.new.len(), r.updated.len(), r.closed.len(), r.unaffected.len(), r.region_types.len(), download_time_ms, store_time_ms);

        Ok(r)
    })
}

/// Download market data of a region
fn download_region(region_id: RegionID,
                   client: esi::Client,
                   uni: universe::Universe,
                   reschedule_channel: channel::Sender<(RegionID, DateTime<Utc>)>)
                   -> Result<store::TaggedOrderVec> {
    // Collect thread's handles for later joins
    // It needs mutable access to the client because of auth and to uni because of the blacklists
    let structure_handle = download_region_structures(region_id, client.clone(), uni);
    let region_handle = download_region_market(region_id, client, reschedule_channel);

    let mut orders: store::TaggedOrderVec = Vec::new();

    // Unwrap thread panics
    let structure_result = structure_handle.join().unwrap();
    let region_result = region_handle.join().unwrap();

    // Unwrap data
    let mut orders_structure = structure_result?;
    let mut orders_region = region_result?;

    // Append and return data
    orders.append(&mut orders_structure);
    orders.append(&mut orders_region);

    Ok(orders)
}

/// Download region metadata and spawn page threads
fn download_region_market(region_id: RegionID,
                          client: esi::Client,
                          reschedule_channel: channel::Sender<(RegionID, DateTime<Utc>)>)
                          -> thread::JoinHandle<Result<store::TaggedOrderVec>> {
    thread::spawn(move || {
        // Todo: Retry
        let metadata = client.get_orders_metadata(region_id)?;

        // Re-schedule self with 3 second safety-margin
        reschedule_channel.send((region_id, metadata.expires + Duration::seconds(3)));

        // Spawn threads for pages
        let mut page_handles: Vec<thread::JoinHandle<Result<Vec<Order>>>> = Vec::new();
        for page in 1..=metadata.pages {
            let handle = download_region_market_page(region_id, client.clone(), page);
            page_handles.push(handle);
        }

        // Collect results
        let mut orders: store::TaggedOrderVec = Vec::new();
        for handle in page_handles {
            // Unpack thread panic
            let thread_result = handle.join().unwrap();

            // Unpack thread result
            let pages = thread_result?;

            for order in pages {
                orders.push((order, metadata.last_modified));
            }
        }

        Ok(orders)
    })
}

/// Download region page (retry 3 times)
fn download_region_market_page(region_id: RegionID,
                               client: esi::Client,
                               page: u32)
                               -> thread::JoinHandle<Result<Vec<Order>>> {
    thread::spawn(move || {
        let mut tries_remaining = 3;
        let mut data: Result<Vec<Order>> = Ok(Vec::new());

        while tries_remaining > 0 {
            tries_remaining -= 1;

            data = client.get_orders(region_id, page);

            if data.is_ok() {
                break;
            }

            thread::sleep(time::Duration::from_millis(1_000));
        }

        data
    })
}

/// Download market data for all of a region's structures
fn download_region_structures(region_id: RegionID,
                              client: esi::Client,
                              uni: universe::Universe)
                              -> thread::JoinHandle<Result<store::TaggedOrderVec>> {
    thread::spawn(move || {
        let structure_id_option = uni.get_public_structures_in_region(region_id);
        let mut orders: store::TaggedOrderVec = Vec::new();

        if let Some(structure_ids) = structure_id_option {
            let mut structure_handles: Vec<thread::JoinHandle<Result<store::TaggedOrderVec>>> =
                Vec::new();

            for structure in structure_ids {
                let handle = download_structure(structure, client.clone(), uni.clone());
                structure_handles.push(handle);
            }

            // Collect results
            for handle in structure_handles {
                // Unpack thread panic
                let thread_result = handle.join().unwrap();

                // Unpack thread result
                let mut structure_orders = thread_result?;

                orders.append(&mut structure_orders);
            }
        }

        Ok(orders)
    })
}

/// Download structure metadata and spawn page threads
fn download_structure(structure_id: LocationID,
                      mut client: esi::Client,
                      uni: universe::Universe)
                      -> thread::JoinHandle<Result<store::TaggedOrderVec>> {
    thread::spawn(move || {
        // Todo: Retry
        let metadata_result = client.get_orders_structure_metadata(structure_id);

        let metadata = match metadata_result {
            Ok(metadata) => metadata,
            Err(e) => {
                // If we got forbidden, blacklist structure and return empty list. On all other errors bail.
                if let errors::ErrorKind::HTTPForbiddenError(ref _e) = *e.kind() {
                    uni.blacklist_structure(structure_id);
                    debug!("Blacklisted structure {}.", structure_id);
                    // Return empty list
                    return Ok(Vec::new());
                }

                bail!(e);
            }
        };

        // Spawn threads for pages
        let mut page_handles: Vec<thread::JoinHandle<Result<Vec<Order>>>> = Vec::new();
        for page in 1..=metadata.pages {
            let handle = download_structure_page(structure_id, client.clone(), page);
            page_handles.push(handle);
        }

        let mut orders: store::TaggedOrderVec = Vec::new();
        for handle in page_handles {
            // Unpack thread panic
            let thread_result = handle.join().unwrap();

            // Unpack thread result
            let pages = thread_result?;

            for order in pages {
                orders.push((order, metadata.last_modified));
            }
        }

        Ok(orders)
    })
}

/// Download structure pages (retry 3 times)
fn download_structure_page(structure_id: LocationID,
                           mut client: esi::Client,
                           page: u32)
                           -> thread::JoinHandle<Result<Vec<Order>>> {
    thread::spawn(move || {
        let mut tries_remaining = 3;
        let mut data: Result<Vec<Order>> = Ok(Vec::new());

        while tries_remaining > 0 {
            tries_remaining -= 1;

            data = client.get_structure_orders(structure_id, page);

            if data.is_ok() {
                break;
            }

            thread::sleep(time::Duration::from_millis(1_000));
        }

        data
    })
}
