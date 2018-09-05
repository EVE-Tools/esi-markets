mod app;
mod config;
mod esi;
mod errors;
mod store;
mod universe;
mod grpc;

extern crate bincode;
extern crate bytes;
extern crate chrono;
extern crate ctrlc;
extern crate fern;
extern crate flate2;
extern crate fnv;
extern crate futures;
extern crate parking_lot;
extern crate prost;
extern crate prost_types;
extern crate rand;
extern crate reqwest;
extern crate serde;
extern crate serde_json;
extern crate time;
extern crate tokio_core;
extern crate tower_service;
extern crate tower_grpc;
extern crate tower_h2;

#[macro_use]
extern crate chan;

#[macro_use]
extern crate log;
extern crate env_logger;

#[macro_use]
extern crate error_chain;

#[macro_use]
extern crate prost_derive;

#[macro_use]
extern crate serde_derive;

use std::env;

use errors::*;

quick_main!(run);

fn run() -> Result<()> {
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{}[{}][{}] {}",
                chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
                record.target(),
                record.level(),
                message
            ))
        })
        .level(log::LevelFilter::Warn)
        .level_for("esi_markets", log::LevelFilter::Debug)
        .chain(std::io::stdout())
        .apply()?;

    // Banner
    info!("\n    ___________ ____     __  ______    ____  __ __ _________________
   / ____/ ___//  _/    /  |/  /   |  / __ \\/ //_// ____/_  __/ ___/
  / __/  \\__ \\ / /_____/ /|_/ / /| | / /_/ / ,<  / __/   / /  \\__ \\
 / /___ ___/ // /_____/ /  / / ___ |/ _, _/ /| |/ /___  / /  ___/ /
/_____//____/___/    /_/  /_/_/  |_/_/ |_/_/ |_/_____/ /_/  /____/\n");

    // Try loading config from environment
    let conf = config::Config::new(env::vars()).chain_err(|| "Unable to load config")?;

    // Run app
    app::run(conf).chain_err(|| "Application error")?;

    Ok(())
}
