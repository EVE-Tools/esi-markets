//! The Order Store A.K.A. The giant map containing the entire market of EVE

use std::fs;
use std::fs::OpenOptions;
use std::hash::{Hash, Hasher};
use std::io::{BufReader, BufWriter};
use std::sync::Arc;
use std::thread;
use std::time;

use bincode::{deserialize_from, serialize_into};
use chrono::prelude::*;
use flate2::bufread::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use fnv::FnvHashMap;
use fnv::FnvHashSet;
use fnv::FnvHasher;
use futures::sync::mpsc::{channel, Sender};
use futures::{Sink, Stream};
use parking_lot::{Mutex, RwLock, RwLockWriteGuard};
use prost::Message;
use prost_types::Timestamp;

use super::errors::*;
use super::esi::types::*;

pub const STORE_PATH: &str = "cache/store";
pub const TEMP_STORE_PATH: &str = "cache/store.part";

/// A vec of orders tagged with external `seen_at`
pub type TaggedOrderVec = Vec<(Order, DateTime<Utc>)>;

/// A serialized order (including `seen_at`).
pub type OrderBlob = Vec<u8>;

/// Main store object wrapped by a `RwLock`.
#[derive(Clone, Debug)]
pub struct Store(Arc<RwLock<InnerStore>>);

/// Result of an update operation.
#[derive(Clone, Debug)]
pub struct UpdateResult {
    pub is_first_run: bool,
    pub new: FnvHashSet<OrderID>,
    pub updated: FnvHashSet<OrderID>,
    pub unaffected: FnvHashSet<OrderID>,
    pub closed: FnvHashSet<OrderID>,
    pub region_types: FnvHashSet<RegionType>,
}

/// Internal storage structure
#[derive(Debug)]
struct InnerStore {
    orders: FnvHashMap<OrderID, StoreObject>,
    regions: FnvHashMap<RegionID, FnvHashSet<OrderID>>,
    types: FnvHashMap<TypeID, FnvHashSet<OrderID>>,
    region_types: FnvHashMap<RegionType, FnvHashSet<OrderID>>,
    update_results_streams: Arc<Mutex<Vec<Sender<Arc<UpdateResult>>>>>,
}

/// Simple order + history
#[derive(Debug, Serialize, Deserialize)]
struct StoreObject {
    order: OrderBlob,
    hash: u64,
    region_id: RegionID,
    type_id: TypeID,
    history: Vec<OrderBlob>,
}

impl Store {
    pub fn new() -> Result<Store> {
        let inner = InnerStore { orders: FnvHashMap::default(),
                                 regions: FnvHashMap::default(),
                                 types: FnvHashMap::default(),
                                 region_types: FnvHashMap::default(),
                                 update_results_streams: Arc::new(Mutex::new(vec![])) };

        let retval = Store(Arc::new(RwLock::new(inner)));

        retval.load_store()?;
        retval.schedule_persistence();

        Ok(retval)
    }

    /// Write local state to disk in regular intervals
    fn schedule_persistence(&self) {
        let clone = self.clone();

        thread::spawn(move || {
            loop {
                thread::sleep(time::Duration::from_secs(60 * 60)); // Hourly

                let clone = clone.clone();
                thread::spawn(move || match clone.persist_store() {
                    Ok(_) => {}
                    Err(e) => warn!("Saving store to disk failed: {}", e),
                });
            }
        });
    }

    /// Save store to disk
    pub fn persist_store(&self) -> Result<()> {
        debug!("Writing store to disk...");
        let store_file = OpenOptions::new().write(true)
                                           .create(true)
                                           .open(TEMP_STORE_PATH)?;

        let writer = BufWriter::new(store_file);
        let mut deflate_writer = DeflateEncoder::new(writer, Compression::fast());

        let res = serialize_into(&mut deflate_writer, &self.0.read().orders);

        if let Err(res) = res {
            bail!(res);
        }

        fs::remove_file(STORE_PATH)?;
        fs::rename(TEMP_STORE_PATH, STORE_PATH)?;
        debug!("Successfully wrote store to disk.");

        Ok(())
    }

    /// Load store from disk or create cache
    fn load_store(&self) -> Result<()> {
        debug!("Loading store from disk...");
        let store_file = OpenOptions::new().read(true)
                                           .write(true)
                                           .create(true)
                                           .open(STORE_PATH)?;

        let reader = BufReader::new(store_file);
        let mut deflate_reader = DeflateDecoder::new(reader);

        let store_result = deserialize_from(&mut deflate_reader);

        if let Err(error) = store_result {
            info!("No orders loaded from disk: {:?}", error);
        } else {
            let mut lock = self.0.write();
            let orders: FnvHashMap<OrderID, StoreObject> = store_result?;
            info!("Loaded {} orders from disk.", orders.len());
            info!("Creating indices...");

            for (id, store_object) in &orders {
                add_to_indices(&mut lock, *id, store_object.type_id, store_object.region_id);
            }

            lock.orders = orders;

            info!("Done creating indices.");
        }

        Ok(())
    }
    /// Store multiple orders in the store. Update if existing
    pub fn store_region(&mut self,
                        region_id: RegionID,
                        orders: TaggedOrderVec)
                        -> Arc<UpdateResult> {
        // Acquire write lock
        // Iterate over orders, serialize (protobuf, append seen_at field later) and hash (build StoreObjects) -> rayon
        //  Check if hash is present
        //      yes -> Is the hash different?
        //          yes -> Persist in Store (and merge history), add to updated
        //          no -> Discard Order, add to unaffected
        //      no -> Persist in store, update indices, add to new
        // prune region
        // Release write lock
        // Return UpdateResult updated (caller can calculate total region_types affected after pruning)
        let mut new: FnvHashSet<OrderID> = FnvHashSet::default();
        let mut updated: FnvHashSet<OrderID> = FnvHashSet::default();
        let mut unaffected: FnvHashSet<OrderID> = FnvHashSet::default();
        let mut region_types: FnvHashSet<RegionType> = FnvHashSet::default();
        let mut all_oids: FnvHashSet<OrderID> = FnvHashSet::default();

        let mut store = self.0.write();

        // Check if there are any orders for this region
        // if not, this is out first run and we avoid flooding new orders
        let is_first_run = !store.regions.contains_key(&region_id);

        for (mut order, last_modified) in orders {
            // Add to all list
            all_oids.insert(order.order_id);

            // Add region ID to order
            order.region_id = region_id;

            // Encode without seen_at
            // TODO: parallelize encode/hash, add seen_at step using rayon?
            let mut hash_blob = Vec::with_capacity(Message::encoded_len(&order));
            Message::encode(&order, &mut hash_blob).unwrap();
            // Hash the blob
            let mut hasher = FnvHasher::default();
            hash_blob.as_slice().hash(&mut hasher);
            let hash = hasher.finish();

            if store.orders.contains_key(&order.order_id) {
                let entry = store.orders.get_mut(&order.order_id).unwrap();

                // Check if hash is different
                if entry.hash != hash {
                    // Set seen_at and reencode
                    order.seen_at = Some(Timestamp::from(time::SystemTime::from(last_modified)));

                    let mut seen_blob = Vec::with_capacity(Message::encoded_len(&order));
                    Message::encode(&order, &mut seen_blob).unwrap();

                    // Update entry
                    updated.insert(order.order_id);
                    region_types.insert((region_id, order.type_id));
                    entry.hash = hash;
                    entry.history.push(seen_blob.clone());
                    entry.order = seen_blob;
                } else {
                    unaffected.insert(order.order_id);
                }
            } else {
                // Not there, new entry
                // Set seen_at and reencode
                order.seen_at = Some(Timestamp::from(time::SystemTime::from(last_modified)));

                let mut seen_blob = Vec::with_capacity(Message::encoded_len(&order));
                Message::encode(&order, &mut seen_blob).unwrap();

                new.insert(order.order_id);
                region_types.insert((region_id, order.type_id));

                store.orders.insert(order.order_id,
                                    StoreObject { hash: hash,
                                                  history: vec![seen_blob.clone()],
                                                  order: seen_blob,
                                                  region_id: region_id,
                                                  type_id: order.type_id });

                add_to_indices(&mut store, order.order_id, order.type_id, region_id);
            }
        }

        let (rt_closed, closed) = prune_region(&mut store, region_id, &all_oids);
        region_types.extend(rt_closed.iter());

        // Push result to stream and return
        let update_result = Arc::new(UpdateResult { is_first_run,
                                                    new,
                                                    updated,
                                                    unaffected,
                                                    closed,
                                                    region_types });

        let mut streams = store.update_results_streams.lock();
        streams.drain_filter(|stream| {
                   match stream.try_send(update_result.clone()) {
                       Ok(()) => false,
                       Err(ref error) if error.is_disconnected() => {
                           debug!("Result stream client disconnected.");
                           true
                       }
                       Err(ref error) if error.is_full() => {
                           warn!("Closing result stream due to backpressured client.");

                           // Close backpressured stream as consumer is not able to keep up
                           let close_result = stream.close();

                           if let Err(error) = close_result {
                               error!("Error while closing result stream: {}", error.to_string());
                           }

                           true
                       }
                       Err(error) => {
                           error!("Unknown stream error while trying to send update result: {}",
                                  error.to_string());
                           true
                       }
                   }
               });
        update_result
    }

    /// Get order's history
    pub fn get_order(&self, order_id: OrderID) -> Vec<OrderBlob> {
        // Acquire read lock
        // Get order's history from store
        // Release read lock
        let store = &self.0.read();

        let mut orders: Vec<OrderBlob> = Vec::new();

        if !store.orders.contains_key(&order_id) {
            return orders;
        }

        for order in &store.orders[&order_id].history {
            orders.push(order.clone());
        }

        orders
    }

    /// Get all orders in a region
    pub fn get_region(&self, region_id: RegionID) -> Vec<OrderBlob> {
        // Acquire read lock
        // Get regions[region_id]
        // Get OIDs from orders, concat to Vec
        // Release read lock
        let store = &self.0.read();
        let oids_region = store.regions.get(&region_id);

        let mut orders: Vec<OrderBlob> = Vec::new();

        if oids_region.is_none() {
            return orders;
        }

        for order_id in oids_region.unwrap() {
            let order_option = store.orders.get(order_id);

            if let Some(order) = order_option {
                orders.push(order.order.clone());
            }
        }

        orders
    }

    /// Get all orders of a type
    pub fn get_type(&self, type_id: TypeID) -> Vec<OrderBlob> {
        // Acquire read lock
        // Get types[region_id]
        // Get OIDs from orders, concat to Vec
        // Release read lock
        let store = &self.0.read();
        let oids_type = store.types.get(&type_id);

        let mut orders: Vec<OrderBlob> = Vec::new();

        if oids_type.is_none() {
            return orders;
        }

        for order_id in oids_type.unwrap() {
            let order_option = store.orders.get(order_id);

            if let Some(order) = order_option {
                orders.push(order.order.clone());
            }
        }

        orders
    }

    /// Get all orders of a type in a region
    pub fn get_region_type(&self, region_type: RegionType) -> Vec<OrderBlob> {
        // Acquire read lock
        // Get region_types[region_type]
        // Get OIDs from orders, concat to Vec
        // Release read lock
        let store = &self.0.read();
        let oids_region_type = store.region_types.get(&region_type);

        let mut orders: Vec<OrderBlob> = Vec::new();

        if oids_region_type.is_none() {
            return orders;
        }

        for order_id in oids_region_type.unwrap() {
            let order_option = store.orders.get(order_id);

            if let Some(order) = order_option {
                orders.push(order.order.clone());
            }
        }

        orders
    }

    /// Get a new future stream containing updateresults
    pub fn get_result_stream(&self) -> Box<Stream<Item = Arc<UpdateResult>, Error = ()>> {
        let store = self.0.write();
        let mut streams = store.update_results_streams.lock();

        debug!("Result stream client connected. Now there are {} active clients.",
               streams.len() + 1);

        let (sender, receiver) = channel::<Arc<UpdateResult>>(10);
        streams.push(sender);
        Box::new(receiver)
    }
}

/// Add new order to indices in store
fn add_to_indices(store: &mut RwLockWriteGuard<InnerStore>,
                  order_id: OrderID,
                  type_id: TypeID,
                  region_id: RegionID) {
    // Add type if unknown
    store.types
         .entry(type_id)
         .or_insert_with(FnvHashSet::default);

    // Add order ID to type index
    store.types.get_mut(&type_id).unwrap().insert(order_id);

    // Add region if unknown
    store.regions
         .entry(region_id)
         .or_insert_with(FnvHashSet::default);

    // Add order ID to region index
    store.regions.get_mut(&region_id).unwrap().insert(order_id);

    // Add region type if unknown
    let region_type: RegionType = (region_id, type_id);

    store.region_types
         .entry(region_type)
         .or_insert_with(FnvHashSet::default);

    // Add order ID to region type index
    store.region_types
         .get_mut(&region_type)
         .unwrap()
         .insert(order_id);
}

/// Clear store of expired orders by comparing OIDs in region, must be done after each full region update!
fn prune_region(store: &mut RwLockWriteGuard<InnerStore>,
                region_id: RegionID,
                ids_in_region: &FnvHashSet<OrderID>)
                -> (FnvHashSet<RegionType>, FnvHashSet<OrderID>) {
    // Acquire write lock
    // Diff ids_in_region with regions[region_id]
    // For those missing in ids_in_region:
    //      Remove from regions[region_id]
    //      Get order from orders, read type_id
    //      Remove from types[type_id]
    //      Remove from region_types[(region_id, type_id)]
    //      Remove from orders
    //
    // regions[region_id] = ids_in_region
    // Release write lock
    // Return

    let mut affected_region_types: FnvHashSet<RegionType> = FnvHashSet::default();
    let mut removed_ids: FnvHashSet<OrderID> = FnvHashSet::default();

    // Sometimes a region stays empty even though an update ran e.g. if the first update failed or if it is a region without orders
    let region_exists = store.regions.contains_key(&region_id);
    if region_exists {
        let orders_to_delete = &store.regions[&region_id].difference(ids_in_region)
                                                         .cloned()
                                                         .collect::<FnvHashSet<OrderID>>();

        for order_id in orders_to_delete {
            let type_id = store.orders[order_id].type_id;
            let region_type: RegionType = (region_id, type_id);

            removed_ids.insert(*order_id);
            affected_region_types.insert(region_type);
            store.regions.get_mut(&region_id).unwrap().remove(order_id);
            store.types.get_mut(&type_id).unwrap().remove(order_id);
            store.region_types
                 .get_mut(&(region_id, type_id))
                 .unwrap()
                 .remove(order_id);
            store.orders.remove(order_id);
        }
    }

    (affected_region_types, removed_ids)
}
