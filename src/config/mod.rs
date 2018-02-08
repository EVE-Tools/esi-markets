use std::env;
use std::string::String;

use super::errors::*;

#[derive(Debug)]
pub struct Config {
    pub refresh_token: String,
    pub client_id: String,
    pub secret_key: String,
    pub grpc_host: String,
}

impl Config {
    // Try to load config from environment
    pub fn new(env: env::Vars) -> Result<Config> {
        let mut refresh_token = String::from("");
        let mut client_id = String::from("");
        let mut secret_key = String::from("");
        let mut grpc_host = String::from("0.0.0.0:43000");
        
        for var in env {
            match var.0.as_ref() {
                "REFRESH_TOKEN" => refresh_token = var.1,
                "CLIENT_ID" => client_id = var.1,
                "SECRET_KEY" => secret_key = var.1,
                "GRPC_HOST" => grpc_host = var.1,
                _ => ()
            }
        }

        // While this code looks redundant, it produces helpful error messages
        if refresh_token.is_empty() {
            bail!("Failed to load configuration variable REFRESH_TOKEN from environment. Try to set it!");
        }

        if client_id.is_empty() {
            bail!("Failed to load configuration variable CLIENT_ID from environment. Try to set it!");
        }

        if secret_key.is_empty() {
            bail!("Failed to load configuration variable SECRET_KEY from environment. Try to set it!");
        }

        Ok(Config { refresh_token, client_id, secret_key, grpc_host })
    }
}
