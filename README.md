# ESI Markets
[![Build Status](https://evetools.semaphoreci.com/badges/esi-markets.svg)](https://evetools.semaphoreci.com/projects/esi-markets) [![Docker Image](https://images.microbadger.com/badges/image/evetools/esi-markets.svg)](https://microbadger.com/images/evetools/esi-markets)

This service for [Element43](https://element-43.com) keeps an in-memory representation of EVE's orderbook which can be queried via gRPC.

The service's gRPC description can be found [here](https://github.com/EVE-Tools/element43/blob/master/services/esiMarkets/esiMarkets.proto).

Issues can be filed [here](https://github.com/EVE-Tools/element43). Pull requests can be made in this repo.

## Installation
Either use the prebuilt Docker images and pass the appropriate env vars (see below), or:

* Install a recent Rust toolchain
* Set the environment variables, for generating ESI tokens you can use [a script in the main repo](https://github.com/EVE-Tools/element43/blob/master/util/tokenfetcher.py)
* Run `cargo run --release` to build and run the release version

## Deployment Info

Environment Variable | Default | Description
--- | --- | ---
REFRESH_TOKEN | - | Your ESI refresh token - must be set!
CLIENT_ID | - | Your ESI client ID - must be set!
SECRET_KEY | - | Your ESI secret key - must be set!
GRPC_HOST | 0.0.0.0:43000 | The host/port esi-markets will listen on