FROM rustlang/rust:nightly-stretch AS build

COPY . /root/esi-markets
RUN cd /root/esi-markets && cargo build --release

FROM debian:stretch-slim

RUN apt-get update && apt-get install -y libssl1.1 ca-certificates openssl && rm -rf /var/lib/apt/lists/*
COPY --from=build /root/esi-markets/target/release/esi-markets /esi-markets

EXPOSE 43000

CMD ["/esi-markets"]
