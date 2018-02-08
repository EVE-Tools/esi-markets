FROM debian:stretch-slim

RUN apt-get update && apt-get install -y libssl1.1 ca-certificates openssl && rm -rf /var/lib/apt/lists/*

COPY ./target/release/esi-markets /esi-markets

EXPOSE 43000

CMD ["/esi-markets"]
