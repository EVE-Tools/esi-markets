pub mod service;

use self::service::{server, GetOrderRequest, GetRegionRequest, GetTypeRequest, GetRegionTypeRequest, GetOrdersResponse, GetRegionTypeUpdateStreamResponse, Empty, Orders, RegionType};
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
        // Get a new stream, take region_types as they become available and convert 
        // the set of store types to a vec of the gRPC types. Then map errors and put stream into a box.
        let stream = self.store.get_result_stream();

        let boxed_stream = Box::new(
            stream.map(|result| {
                GetRegionTypeUpdateStreamResponse {
                    region_types: result.region_types
                        .iter()
                        .map(|(region_id, type_id)| {
                            RegionType {
                                region_id: *region_id, 
                                type_id: *type_id 
                            }
                        })
                        .collect(),
                }
            })
            .map_err(|_| Error::from(())));

        future::ok(Response::new(boxed_stream))
    }
}

pub fn run_server(store: store::Store, grpc_host: &str) {
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