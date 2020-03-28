use prost_types::Timestamp;

// An SSO response
#[derive(Clone, Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    pub refresh_token: String,
}

/// Order struct from ESI
#[derive(Clone, Deserialize, Serialize, PartialEq, Message)]
pub struct Order {
    /// The order's ID
    #[prost(uint64, tag = "1")]
    pub order_id: u64,
    /// The order's type's ID
    #[prost(uint64, tag = "2")]
    pub type_id: u64,
    /// The order's type's ID
    #[prost(uint64, tag = "3")]
    #[serde(skip_deserializing)]
    pub region_id: u64,
    /// ID of the order's location/station
    #[prost(uint64, tag = "4")]
    pub location_id: u64,
    /// Initital number of items of the order
    #[prost(uint64, tag = "5")]
    pub volume_total: u64,
    /// Number of items remaining
    #[prost(uint64, tag = "6")]
    pub volume_remain: u64,
    /// Minimum volume to betraded for this order
    #[prost(uint64, tag = "7")]
    pub min_volume: u64,
    /// The price the type is bought/sold for
    #[prost(double, tag = "8")]
    pub price: f64,
    /// True: Bid/buy order | False: ask/sell order
    #[prost(bool, tag = "9")]
    pub is_buy_order: bool,
    /// Defines how long the order exists after creation
    #[prost(int32, tag = "10")]
    pub duration: i32,
    /// Order's range
    #[prost(string, tag = "11")]
    pub range: String,
    /// Date the order was issued
    #[prost(message, optional, tag = "12")]
    #[serde(with = "prost_date")]
    pub issued: Option<Timestamp>,
    /// When the order was last seen in this state
    #[prost(message, optional, tag = "13")]
    #[serde(skip_deserializing, with = "prost_date")]
    pub seen_at: Option<Timestamp>,
}

/// Custom decoding for interop between Prost and Serde
mod prost_date {
    use chrono::DateTime;
    use chrono::Utc;
    use prost_types::Timestamp;
    use serde::{self, Deserialize, Deserializer, Serializer};
    use std::convert::TryFrom;
    use std::time;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Timestamp>, D::Error>
        where D: Deserializer<'de>
    {
        let s = String::deserialize(deserializer)?;
        let d = DateTime::parse_from_rfc3339(&s).map_err(serde::de::Error::custom)?;
        let st = time::SystemTime::from(d);
        Ok(Some(Timestamp::from(st)))
    }

    pub fn serialize<S>(date: &Option<Timestamp>, serializer: S) -> Result<S::Ok, S::Error>
        where S: Serializer
    {
        let a: Timestamp = date.clone().unwrap();
        let st: Result<time::SystemTime, time::Duration> = TryFrom::try_from(a);
        let st = st.unwrap();
        let rfc_string = DateTime::<Utc>::from(st).to_rfc3339();

        serializer.serialize_str(&rfc_string)
    }
}

/// A combination of `region_id` and `type_id`.
pub type RegionType = (RegionID, TypeID);

/// A region's id
pub type RegionID = u64;

/// A type's id
pub type TypeID = u64;

/// An order's id
pub type OrderID = u64;

/// A location's id
pub type LocationID = u64;
