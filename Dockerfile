FROM rustlang/rust:nightly AS build

COPY . .
RUN cargo build -j 1 --verbose --release

FROM debian:stretch-slim

RUN apt-get update && apt-get install -y libssl1.1 ca-certificates openssl && rm -rf /var/lib/apt/lists/*
COPY --from=build ./target/release/esi-markets /esi-markets

EXPOSE 43000

CMD ["/esi-markets"]
