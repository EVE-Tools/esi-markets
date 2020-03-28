pub mod types;

use std::collections::HashMap;
use std::str;
use std::sync::Arc;
use std::thread;

use chrono::prelude::*;
use chrono::Duration;
use parking_lot::RwLock;

use super::errors;
use super::errors::*;

pub const USER_AGENT: &str = "ESI-Markets (element-43.com)";

/// A struct containing the context for ESI's oAuth
#[derive(Clone, Debug)]
pub struct OAuthContext {
    client_id: String,
    secret_key: String,
    refresh_token: String,
    access_token: Option<String>,
    access_until: DateTime<Utc>,
}

impl OAuthContext {
    pub fn new(client_id: String, secret_key: String, refresh_token: String) -> OAuthContext {
        OAuthContext { client_id,
                       secret_key,
                       refresh_token,
                       access_token: None,
                       access_until: Utc::now() }
    }
}

/// Metadata about a set of orders
#[derive(Clone, Debug)]
pub struct MarketMetadata {
    pub pages: u32,
    pub expires: DateTime<Utc>,
    pub last_modified: DateTime<Utc>,
}

/// A client for performing calls to the API.
#[derive(Clone, Debug)]
pub struct Client(Arc<RwLock<InnerClient>>);

#[derive(Debug)]
struct InnerClient {
    http: reqwest::blocking::Client,
    locked_until: DateTime<Utc>,
    oauth_context: OAuthContext,
}

impl Client {
    pub fn new(oauth_context: OAuthContext) -> Client {
        let client = reqwest::blocking::Client::builder().max_idle_per_host(10)
                                                         .user_agent(USER_AGENT)
                                                         .gzip(true)
                                                         .build()
                                                         .unwrap();
        let inner_client = InnerClient { http: client,
                                         locked_until: Utc::now(),
                                         oauth_context };

        Client(Arc::new(RwLock::new(inner_client)))
    }

    pub fn get_orders(&self, region_id: types::RegionID, page: u32) -> Result<Vec<types::Order>> {
        let url = format!("https://esi.evetech.net/v1/markets/{}/orders/?page={}",
                          region_id, page);
        let data = unwrap_json(self.get(&url)?)?;

        Ok(data)
    }

    pub fn get_region_ids(&self) -> Result<Vec<types::RegionID>> {
        let url = "https://esi.evetech.net/v1/universe/regions/".to_string();
        let data = unwrap_json(self.get(&url)?)?;

        Ok(data)
    }

    pub fn get_market_structure_ids(&self) -> Result<Vec<types::LocationID>> {
        let url = "https://esi.evetech.net/v1/universe/structures/?filter=market".to_string();
        let data = unwrap_json(self.get(&url)?)?;

        Ok(data)
    }

    pub fn get_structure_orders(&mut self,
                                structure_id: u64,
                                page: u32)
                                -> Result<Vec<types::Order>> {
        let url = format!("https://esi.evetech.net/v1/markets/structures/{}/?page={}",
                          structure_id, page);
        let data = unwrap_json(self.get_auth(&url)?)?;

        Ok(data)
    }

    /// Return metadata for orders such as number of pages and date generated
    pub fn get_orders_metadata(&self, region_id: types::RegionID) -> Result<MarketMetadata> {
        let url = format!("https://esi.evetech.net/v1/markets/{}/orders/?page=1",
                          region_id);
        let response = &self.get(&url)?;

        self.handle_get_response(response)?;

        let pages = header_as_number("x-pages", response)?;
        let expires = header_as_datetime("expires", response)?;
        let last_modified = header_as_datetime("last-modified", response)?;

        Ok(MarketMetadata { pages,
                            expires,
                            last_modified })
    }

    /// Return metadata for orders such as number of pages and date generated
    pub fn get_orders_structure_metadata(&mut self, structure_id: u64) -> Result<MarketMetadata> {
        let url = format!("https://esi.evetech.net/v1/markets/structures/{}/?page=1",
                          structure_id);
        let response = &self.get_auth(&url)?;
        let forbidden = response.status() == reqwest::StatusCode::FORBIDDEN;

        if forbidden {
            bail!(errors::ErrorKind::HTTPForbiddenError(format!("Structure {} is forbidden!",
                                                                structure_id)));
        }

        self.handle_get_response(response)?;

        let pages = header_as_number("x-pages", response)?;
        let expires = header_as_datetime("expires", response)?;
        let last_modified = header_as_datetime("last-modified", response)?;

        Ok(MarketMetadata { pages,
                            expires,
                            last_modified })
    }

    /// Simple HTTP GET helper for common requests
    fn get(&self, url: &str) -> Result<reqwest::blocking::Response> {
        self.limit_errors()?;
        let client = &self.0.read().http.clone();

        let resp = client.get(url).header("connection", "keep-alive").send()?;

        self.handle_get_response(&resp)?;

        Ok(resp)
    }

    /// Simple HTTP GET helper for common requests requiring authentication
    fn get_auth(&mut self, url: &str) -> Result<reqwest::blocking::Response> {
        self.limit_errors()?;
        self.check_auth()?;

        let client = &self.0.read().http.clone();
        let auth_ctx = self.0.read().oauth_context.clone();

        let resp = client.get(url)
                         .header("connection", "keep-alive")
                         .bearer_auth(auth_ctx.access_token.unwrap())
                         .send()?;

        self.handle_get_response(&resp)?;

        Ok(resp)
    }

    /// Check if auth info is valid, refesh `access_token` if needed
    fn check_auth(&mut self) -> Result<()> {
        let auth_valid = {
            let access_until = self.0.read().oauth_context.access_until;
            let has_token = self.0.read().oauth_context.access_token.is_some();
            // Refresh token before expiry
            has_token && access_until.signed_duration_since(Utc::now()) < Duration::seconds(60)
        };

        if !auth_valid {
            // Get write-lock
            let mut lock = self.0.write();

            // Now that we're alone check again, as there could be multiple of these requests in parallel and this thread could have been blocked waiting for a lock all the time
            if lock.oauth_context
                   .access_until
                   .signed_duration_since(Utc::now())
               < Duration::seconds(60)
            {
                info!("Refreshing auth token.");
                let client_id = lock.oauth_context.client_id.clone();
                let secret_key = lock.oauth_context.secret_key.clone();

                let mut body = HashMap::new();
                body.insert("grant_type", "refresh_token");
                body.insert("refresh_token", &lock.oauth_context.refresh_token);

                // Load auth, acquire write lock, check again, then write new data
                let response = lock.http
                                   .post("https://login.eveonline.com/v2/oauth/token")
                                   .basic_auth(client_id, Some(secret_key))
                                   .form(&body)
                                   .send()?;

                let auth_data: types::TokenResponse = unwrap_json(response)?;

                lock.oauth_context.access_token = Some(auth_data.access_token);
                lock.oauth_context.access_until =
                    Utc::now() + Duration::seconds(auth_data.expires_in);
            }
        }

        Ok(())
    }

    /// Fail requests if client is blocked due to hitting the error limit. Block thread/wait if less than 30 seconds remaining until reset.
    fn limit_errors(&self) -> Result<()> {
        let locked_until = self.0.read().locked_until;

        if locked_until > Utc::now() {
            let difference = locked_until.signed_duration_since(Utc::now());

            if difference > Duration::seconds(30) {
                bail!(errors::ErrorKind::ESIErrorLimitError("Skipped request as we're blocked by ESI!".to_owned()));
            }

            thread::sleep(difference.to_std()?);
        }

        Ok(())
    }

    /// Inspect response for errors, especially for ESI-blocks
    // FIXME: properly handle non-200 responses!
    fn handle_get_response(&self, resp: &reqwest::blocking::Response) -> Result<()> {
        let errors_remaining = header_as_number("x-esi-error-limit-remain", resp)?;
        let reset_seconds = header_as_number("x-esi-error-limit-reset", resp)?;
        let reset_window_at = Utc::now() + Duration::seconds(reset_seconds.into());
        let limit_present = resp.headers().get("x-esi-error-limited").is_some();
        let status_code = resp.status().as_u16();

        // Throttle request
        if limit_present || status_code == 420 || errors_remaining < 10 {
            self.0.write().locked_until = reset_window_at;
        }

        // Bail or continue depending on cause
        if limit_present {
            bail!(errors::ErrorKind::ESIErrorLimitError("Request blocked by ESI: Blocking header present. Throttling requests.".to_owned()));
        } else if status_code == 420 {
            bail!(errors::ErrorKind::ESIErrorLimitError("Request blocked by ESI: Server returned code 420. Throttling requests.".to_owned()));
        } else if errors_remaining < 10 {
            warn!("Remaining ESI error limit below watermark: Throttling requests.");
        } else if status_code != 200 && status_code != 403 {
            bail!(errors::ErrorKind::ESIErrorLimitError(format!("ESI returned error: {:?}", resp)));
        }

        Ok(())
    }
}

/// Try to return an arbitrary header as u32
fn header_as_number(name: &str, resp: &reqwest::blocking::Response) -> Result<u32> {
    let header = resp.headers()
                     .get(name)
                     .ok_or_else(|| Error::from(format!("Response missing the {} header", name)))?
                     .to_str()?;

    let num = header.parse()
                    .chain_err(|| format!("Could not parse {} header", name))?;

    Ok(num)
}

/// Try to return an arbitrary header as UTC datetime
fn header_as_datetime(name: &str, resp: &reqwest::blocking::Response) -> Result<DateTime<Utc>> {
    let header = resp.headers()
                     .get(name)
                     .ok_or_else(|| Error::from(format!("Response missing the {} header", name)))?
                     .to_str()?;

    let timestamp = DateTime::parse_from_rfc2822(header).chain_err(|| {
                                                            format!("Could not parse {} header",
                                                                    name)
                                                        })?;
    let utc_timestamp = timestamp.with_timezone(&Utc);

    Ok(utc_timestamp)
}

/// Parse JSON via string for performance reasons, see: <https://github.com/serde-rs/json/issues/160>
fn unwrap_json<T>(resp: reqwest::blocking::Response) -> Result<T>
    where T: serde::de::DeserializeOwned
{
    let body = resp.text()?;
    let data = serde_json::from_str(&body)?;

    Ok(data)
}
