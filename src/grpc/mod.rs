use bytes::Bytes;
use prost::Message;
use tokio::sync::mpsc;
use tonic::{transport::Server, Request, Response, Status};

use esi_markets::esi_markets_server::{EsiMarkets, EsiMarketsServer};
use esi_markets::{
    GetOrderRequest, GetOrdersResponse, GetRegionRequest, GetRegionTypeRequest,
    GetRegionTypeUpdateStreamResponse, GetTypeRequest, Order,
};

use super::store;

pub mod esi_markets {
    tonic::include_proto!("esi_markets");
}

#[derive(Clone, Debug)]
pub struct MarketServer {
    store: store::Store,
}

#[tonic::async_trait]
impl EsiMarkets for MarketServer {
    async fn get_order(&self,
                       request: Request<GetOrderRequest>)
                       -> Result<Response<GetOrdersResponse>, Status> {
        let blobs = self.store.get_order(request.get_ref().order_id);
        let orders: Vec<Order> = blobs.iter()
                                      .map(|blob| decode_order_blob(blob.to_vec()))
                                      .collect();
        Ok(Response::new(GetOrdersResponse { orders }))
    }
    async fn get_region(&self,
                        request: Request<GetRegionRequest>)
                        -> Result<Response<GetOrdersResponse>, Status> {
        let blobs = self.store.get_region(request.get_ref().region_id);
        let orders: Vec<Order> = blobs.iter()
                                      .map(|blob| decode_order_blob(blob.to_vec()))
                                      .collect();
        Ok(Response::new(GetOrdersResponse { orders }))
    }
    async fn get_type(&self,
                      request: Request<GetTypeRequest>)
                      -> Result<Response<GetOrdersResponse>, Status> {
        let blobs = self.store.get_type(request.get_ref().type_id);
        let orders: Vec<Order> = blobs.iter()
                                      .map(|blob| decode_order_blob(blob.to_vec()))
                                      .collect();
        Ok(Response::new(GetOrdersResponse { orders }))
    }
    async fn get_region_type(&self,
                             request: Request<GetRegionTypeRequest>)
                             -> Result<Response<GetOrdersResponse>, Status> {
        let blobs = self.store
                        .get_region_type((request.get_ref().region_id, request.get_ref().type_id));
        let orders: Vec<Order> = blobs.iter()
                                      .map(|blob| decode_order_blob(blob.to_vec()))
                                      .collect();
        Ok(Response::new(GetOrdersResponse { orders }))
    }
    #[doc = "Server streaming response type for the GetRegionTypeUpdateStream method."]
    type GetRegionTypeUpdateStreamStream =
        mpsc::Receiver<Result<GetRegionTypeUpdateStreamResponse, Status>>;
    async fn get_region_type_update_stream(
        &self,
        _request: Request<()>)
        -> Result<Response<Self::GetRegionTypeUpdateStreamStream>, Status> {
        // Get a new stream, take region_types as they become available and convert
        // the set of store types to a vec of the gRPC types. Then map errors and put stream into a box.
        let stream = self.store.get_result_stream();

        Ok(Response::new(stream))
    }
}

fn decode_order_blob(blob: Vec<u8>) -> Order {
    Message::decode(Bytes::from(blob)).unwrap()
}

#[tokio::main]
pub async fn run_server(store: store::Store,
                        grpc_host: &str)
                        -> Result<(), Box<dyn std::error::Error>> {
    let addr = grpc_host.parse().unwrap();
    info!("RPC server listening on: {}", grpc_host);

    let service = MarketServer { store };
    let svc = EsiMarketsServer::new(service);

    Server::builder().add_service(svc).serve(addr).await?;

    Ok(())
}
