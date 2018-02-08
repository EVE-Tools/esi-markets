pub mod service;

use self::service::{server, GetOrderRequest, GetRegionRequest, GetTypeRequest, GetRegionTypeRequest, GetOrdersResponse, GetRegionTypeUpdateStreamResponse, Empty, Orders};
use super::store;

use futures::{future, Future, Stream};
use tokio_core::net::TcpListener;
use tokio_core::reactor::Core;
use tower_h2::Server;
use tower_grpc::{Request, Response, Error};
use prost;

#[derive(Clone, Debug)]
struct MarketServer {
    store: store::Store
}

// Prepend tags for repeated field
fn encode_orders(blobs: Vec<store::OrderBlob>) -> GetOrdersResponse {
    let mut response: Orders = Vec::new();
    for mut blob in blobs {
        let mut prefix = vec![10u8];
        let mut lenbuf = Vec::with_capacity(prost::encoding::encoded_len_varint(blob.len() as u64));
        prost::encoding::encode_varint(blob.len() as u64, &mut lenbuf);
        prefix.append(&mut lenbuf);
        prefix.append(&mut blob);
        response.append(&mut prefix);
    }

    GetOrdersResponse{orders: response}
}

impl server::EsiMarkets for MarketServer {
    type GetOrderFuture = future::FutureResult<Response<GetOrdersResponse>, Error>;
    type GetRegionFuture = future::FutureResult<Response<GetOrdersResponse>, Error>;
    type GetTypeFuture = future::FutureResult<Response<GetOrdersResponse>, Error>;
    type GetRegionTypeFuture = future::FutureResult<Response<GetOrdersResponse>, Error>;
    type GetRegionTypeUpdateStreamStream = Box<Stream<Item = GetRegionTypeUpdateStreamResponse, Error = Error>>;
    type GetRegionTypeUpdateStreamFuture = future::FutureResult<Response<Self::GetRegionTypeUpdateStreamStream>, Error>;

    fn get_order(&mut self, request: Request<GetOrderRequest>) -> Self::GetOrderFuture {
        let blobs  = self.store.get_order(request.get_ref().order_id);
        future::ok(Response::new(encode_orders(blobs)))
    }

    fn get_region(&mut self, request: Request<GetRegionRequest>) -> Self::GetRegionFuture {
        let blobs  = self.store.get_region(request.get_ref().region_id);
        future::ok(Response::new(encode_orders(blobs)))
    }

    fn get_type(&mut self, request: Request<GetTypeRequest>) -> Self::GetTypeFuture {
        let blobs = self.store.get_type(request.get_ref().type_id);
        future::ok(Response::new(encode_orders(blobs)))
    }

    fn get_region_type(&mut self, request: Request<GetRegionTypeRequest>) -> Self::GetRegionTypeFuture {
        let blobs = self.store.get_region_type((request.get_ref().region_id, request.get_ref().type_id));
        future::ok(Response::new(encode_orders(blobs)))
    }

    fn get_region_type_update_stream(&mut self, _request: Request<Empty>) -> Self::GetRegionTypeUpdateStreamFuture {
        // FIXME: Implement
        use futures::sync::mpsc;

        let (_, rx) = mpsc::channel(1);
        let rx = rx.map_err(|_| unimplemented!());

        future::ok(Response::new(Box::new(rx)))
    }
}

pub fn run_server(store: store::Store, grpc_host: String) {
    let _ = ::env_logger::init();

    let mut core = Core::new().unwrap();
    let reactor = core.handle();

    let new_service = server::EsiMarketsServer::new(MarketServer{store: store});

    let h2 = Server::new(new_service, Default::default(), reactor.clone());

    let addr = grpc_host.parse().unwrap();
    let bind = TcpListener::bind(&addr, &reactor).expect("bind");

    let serve = bind.incoming()
        .fold((h2, reactor), |(h2, reactor), (sock, _)| {
            if let Err(e) = sock.set_nodelay(true) {
                return Err(e);
            }

            let serve = h2.serve(sock);
            reactor.spawn(serve.map_err(|e| error!("h2 error: {:?}", e)));

            Ok((h2, reactor))
        });

    core.run(serve).unwrap();
}