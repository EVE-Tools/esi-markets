use bytes::{Buf, BufMut};
use prost::encoding::*;
use prost::{DecodeError, Message};

#[derive(Clone, PartialEq, Message)]
pub struct Empty {}

#[derive(Clone, PartialEq, Message)]
pub struct GetOrderRequest {
    /// Fetch recorded history of an order
    #[prost(uint64, tag = "1")]
    pub order_id: u64,
}
#[derive(Clone, PartialEq, Message)]
pub struct GetRegionRequest {
    /// Defines which region the data is fetched for
    #[prost(uint64, tag = "1")]
    pub region_id: u64,
}
#[derive(Clone, PartialEq, Message)]
pub struct GetTypeRequest {
    /// Defines which type the data is fetched for
    #[prost(uint64, tag = "1")]
    pub type_id: u64,
}
#[derive(Clone, PartialEq, Message)]
pub struct GetRegionTypeRequest {
    /// Defines which region the data is fetched for
    #[prost(uint64, tag = "1")]
    pub region_id: u64,
    /// Defines which type the data is fetched for
    #[prost(uint64, tag = "2")]
    pub type_id: u64,
}
#[derive(Clone, PartialEq, Message)]
pub struct GetRegionTypeUpdateStreamResponse {
    /// Region/tye pairs affected by update
    #[prost(message, repeated, tag = "1")]
    pub region_types: ::std::vec::Vec<RegionType>,
}
#[derive(Clone, PartialEq, Message)]
pub struct RegionType {
    /// The update's region's ID
    #[prost(uint64, tag = "1")]
    pub region_id: u64,
    /// The update's type's ID
    #[prost(uint64, tag = "2")]
    pub type_id: u64,
}

// Wrap orders type as we keep pre-encoded protobufs in store,
// thus we can use our own encoder which simply writes the raw data into a buffer.
pub type Orders = Vec<u8>;

#[derive(Debug)]
pub struct GetOrdersResponse {
    pub orders: Orders,
}

impl Message for GetOrdersResponse {
    fn encode_raw<B>(&self, buf: &mut B)
        where B: BufMut
    {
        buf.put_slice(&self.orders);
    }
    fn merge_field<B>(&mut self, buf: &mut B) -> Result<(), DecodeError>
        where B: Buf
    {
        let (tag, wire_type) = decode_key(buf)?;
        if tag == 1 {
            bytes::merge(wire_type, &mut self.orders, buf)
        } else {
            skip_field(wire_type, buf)
        }
    }
    fn encoded_len(&self) -> usize {
        if !self.orders.is_empty() {
            bytes::encoded_len(1, &self.orders)
        } else {
            0
        }
    }
    fn clear(&mut self) {
        self.orders.clear();
    }
}

pub mod server {
    use super::{
        Empty, GetOrderRequest, GetOrdersResponse, GetRegionRequest, GetRegionTypeRequest,
        GetRegionTypeUpdateStreamResponse, GetTypeRequest,
    };
    use tower_grpc::codegen::server::*;

    // Redefine the try_ready macro so that it doesn't need to be explicitly
    // imported by the user of this generated code.
    macro_rules! try_ready {
        ($e:expr) => {
            match $e {
                Ok(futures::Async::Ready(t)) => t,
                Ok(futures::Async::NotReady) => return Ok(futures::Async::NotReady),
                Err(e) => return Err(From::from(e)),
            }
        };
    }

    pub trait EsiMarkets: Clone {
        type GetOrderFuture: futures::Future<Item = grpc::Response<GetOrdersResponse>,
                                             Error = grpc::Error>;
        type GetRegionFuture: futures::Future<Item = grpc::Response<GetOrdersResponse>,
                                              Error = grpc::Error>;
        type GetTypeFuture: futures::Future<Item = grpc::Response<GetOrdersResponse>,
                                            Error = grpc::Error>;
        type GetRegionTypeFuture: futures::Future<Item = grpc::Response<GetOrdersResponse>,
                                                  Error = grpc::Error>;
        type GetRegionTypeUpdateStreamStream: futures::Stream<Item = GetRegionTypeUpdateStreamResponse, Error = grpc::Error>;
        type GetRegionTypeUpdateStreamFuture: futures::Future<Item = grpc::Response<Self::GetRegionTypeUpdateStreamStream>, Error = grpc::Error>;

        fn get_order(&mut self, request: grpc::Request<GetOrderRequest>) -> Self::GetOrderFuture;

        fn get_region(&mut self, request: grpc::Request<GetRegionRequest>)
                      -> Self::GetRegionFuture;

        fn get_type(&mut self, request: grpc::Request<GetTypeRequest>) -> Self::GetTypeFuture;

        fn get_region_type(&mut self,
                           request: grpc::Request<GetRegionTypeRequest>)
                           -> Self::GetRegionTypeFuture;

        fn get_region_type_update_stream(&mut self,
                                         request: grpc::Request<Empty>)
                                         -> Self::GetRegionTypeUpdateStreamFuture;
    }

    #[derive(Debug, Clone)]
    pub struct EsiMarketsServer<T> {
        esi_markets: T,
    }

    impl<T> EsiMarketsServer<T> where T: EsiMarkets
    {
        pub fn new(esi_markets: T) -> Self {
            Self { esi_markets }
        }
    }

    impl<T> tower::Service for EsiMarketsServer<T> where T: EsiMarkets
    {
        type Request = http::Request<tower_h2::RecvBody>;
        type Response = http::Response<esi_markets::ResponseBody<T>>;
        type Error = h2::Error;
        type Future = esi_markets::ResponseFuture<T>;

        fn poll_ready(&mut self) -> futures::Poll<(), Self::Error> {
            Ok(().into())
        }

        fn call(&mut self, request: Self::Request) -> Self::Future {
            use self::esi_markets::Kind::*;

            match request.uri().path() {
                "/esiMarkets.ESIMarkets/GetOrder" => {
                    let service = esi_markets::methods::GetOrder(self.esi_markets.clone());
                    let response = grpc::Grpc::unary(service, request);
                    esi_markets::ResponseFuture { kind: Ok(GetOrder(response)) }
                }
                "/esiMarkets.ESIMarkets/GetRegion" => {
                    let service = esi_markets::methods::GetRegion(self.esi_markets.clone());
                    let response = grpc::Grpc::unary(service, request);
                    esi_markets::ResponseFuture { kind: Ok(GetRegion(response)) }
                }
                "/esiMarkets.ESIMarkets/GetType" => {
                    let service = esi_markets::methods::GetType(self.esi_markets.clone());
                    let response = grpc::Grpc::unary(service, request);
                    esi_markets::ResponseFuture { kind: Ok(GetType(response)) }
                }
                "/esiMarkets.ESIMarkets/GetRegionType" => {
                    let service = esi_markets::methods::GetRegionType(self.esi_markets.clone());
                    let response = grpc::Grpc::unary(service, request);
                    esi_markets::ResponseFuture { kind: Ok(GetRegionType(response)) }
                }
                "/esiMarkets.ESIMarkets/GetRegionTypeUpdateStream" => {
                    let service =
                        esi_markets::methods::GetRegionTypeUpdateStream(self.esi_markets.clone());
                    let response = grpc::Grpc::server_streaming(service, request);
                    esi_markets::ResponseFuture { kind: Ok(GetRegionTypeUpdateStream(response)) }
                }
                _ => esi_markets::ResponseFuture { kind: Err(grpc::Status::UNIMPLEMENTED) },
            }
        }
    }

    impl<T> tower::NewService for EsiMarketsServer<T> where T: EsiMarkets
    {
        type Request = http::Request<tower_h2::RecvBody>;
        type Response = http::Response<esi_markets::ResponseBody<T>>;
        type Error = h2::Error;
        type Service = Self;
        type InitError = h2::Error;
        type Future = futures::FutureResult<Self::Service, Self::Error>;

        fn new_service(&self) -> Self::Future {
            futures::ok(self.clone())
        }
    }

    pub mod esi_markets {
        use super::EsiMarkets;
        use tower_grpc::codegen::server::*;

        pub struct ResponseFuture<T>
        where T: EsiMarkets,
        {
            pub(super) kind: Result<Kind<
                grpc::unary::ResponseFuture<methods::GetOrder<T>, tower_h2::RecvBody>,
                grpc::unary::ResponseFuture<methods::GetRegion<T>, tower_h2::RecvBody>,
                grpc::unary::ResponseFuture<methods::GetType<T>, tower_h2::RecvBody>,
                grpc::unary::ResponseFuture<methods::GetRegionType<T>, tower_h2::RecvBody>,
                grpc::server_streaming::ResponseFuture<methods::GetRegionTypeUpdateStream<T>, tower_h2::RecvBody>,
            >, grpc::Status>,
        }

        impl<T> futures::Future for ResponseFuture<T> where T: EsiMarkets
        {
            type Item = http::Response<ResponseBody<T>>;
            type Error = h2::Error;

            fn poll(&mut self) -> futures::Poll<Self::Item, Self::Error> {
                use self::Kind::*;

                match self.kind {
                    Ok(GetOrder(ref mut fut)) => {
                        let response = try_ready!(fut.poll());
                        let (head, body) = response.into_parts();
                        let body = ResponseBody { kind: Ok(GetOrder(body)) };
                        let response = http::Response::from_parts(head, body);
                        Ok(response.into())
                    }
                    Ok(GetRegion(ref mut fut)) => {
                        let response = try_ready!(fut.poll());
                        let (head, body) = response.into_parts();
                        let body = ResponseBody { kind: Ok(GetRegion(body)) };
                        let response = http::Response::from_parts(head, body);
                        Ok(response.into())
                    }
                    Ok(GetType(ref mut fut)) => {
                        let response = try_ready!(fut.poll());
                        let (head, body) = response.into_parts();
                        let body = ResponseBody { kind: Ok(GetType(body)) };
                        let response = http::Response::from_parts(head, body);
                        Ok(response.into())
                    }
                    Ok(GetRegionType(ref mut fut)) => {
                        let response = try_ready!(fut.poll());
                        let (head, body) = response.into_parts();
                        let body = ResponseBody { kind: Ok(GetRegionType(body)) };
                        let response = http::Response::from_parts(head, body);
                        Ok(response.into())
                    }
                    Ok(GetRegionTypeUpdateStream(ref mut fut)) => {
                        let response = try_ready!(fut.poll());
                        let (head, body) = response.into_parts();
                        let body = ResponseBody { kind: Ok(GetRegionTypeUpdateStream(body)) };
                        let response = http::Response::from_parts(head, body);
                        Ok(response.into())
                    }
                    Err(ref status) => {
                        let body = ResponseBody { kind: Err(status.clone()) };
                        Ok(grpc::Response::new(body).into_http().into())
                    }
                }
            }
        }

        pub struct ResponseBody<T>
        where T: EsiMarkets,
        {
            pub(super) kind: Result<Kind<
                grpc::Encode<grpc::unary::Once<<methods::GetOrder<T> as grpc::UnaryService>::Response>>,
                grpc::Encode<grpc::unary::Once<<methods::GetRegion<T> as grpc::UnaryService>::Response>>,
                grpc::Encode<grpc::unary::Once<<methods::GetType<T> as grpc::UnaryService>::Response>>,
                grpc::Encode<grpc::unary::Once<<methods::GetRegionType<T> as grpc::UnaryService>::Response>>,
                grpc::Encode<<methods::GetRegionTypeUpdateStream<T> as grpc::ServerStreamingService>::ResponseStream>,
            >, grpc::Status>,
        }

        impl<T> tower_h2::Body for ResponseBody<T> where T: EsiMarkets
        {
            type Data = bytes::Bytes;

            fn is_end_stream(&self) -> bool {
                use self::Kind::*;

                match self.kind {
                    Ok(GetOrder(ref v)) => v.is_end_stream(),
                    Ok(GetRegion(ref v)) => v.is_end_stream(),
                    Ok(GetType(ref v)) => v.is_end_stream(),
                    Ok(GetRegionType(ref v)) => v.is_end_stream(),
                    Ok(GetRegionTypeUpdateStream(ref v)) => v.is_end_stream(),
                    Err(_) => true,
                }
            }

            fn poll_data(&mut self) -> futures::Poll<Option<Self::Data>, h2::Error> {
                use self::Kind::*;

                match self.kind {
                    Ok(GetOrder(ref mut v)) => v.poll_data(),
                    Ok(GetRegion(ref mut v)) => v.poll_data(),
                    Ok(GetType(ref mut v)) => v.poll_data(),
                    Ok(GetRegionType(ref mut v)) => v.poll_data(),
                    Ok(GetRegionTypeUpdateStream(ref mut v)) => v.poll_data(),
                    Err(_) => Ok(None.into()),
                }
            }

            fn poll_trailers(&mut self) -> futures::Poll<Option<http::HeaderMap>, h2::Error> {
                use self::Kind::*;

                match self.kind {
                    Ok(GetOrder(ref mut v)) => v.poll_trailers(),
                    Ok(GetRegion(ref mut v)) => v.poll_trailers(),
                    Ok(GetType(ref mut v)) => v.poll_trailers(),
                    Ok(GetRegionType(ref mut v)) => v.poll_trailers(),
                    Ok(GetRegionTypeUpdateStream(ref mut v)) => v.poll_trailers(),
                    Err(ref status) => {
                        let mut map = http::HeaderMap::new();
                        map.insert("grpc-status", status.to_header_value());
                        Ok(Some(map).into())
                    }
                }
            }
        }

        #[derive(Debug, Clone)]
        pub(super) enum Kind<GetOrder, GetRegion, GetType, GetRegionType, GetRegionTypeUpdateStream> {
            GetOrder(GetOrder),
            GetRegion(GetRegion),
            GetType(GetType),
            GetRegionType(GetRegionType),
            GetRegionTypeUpdateStream(GetRegionTypeUpdateStream),
        }

        pub mod methods {
            use super::super::{
                Empty, EsiMarkets, GetOrderRequest, GetOrdersResponse, GetRegionRequest,
                GetRegionTypeRequest, GetTypeRequest,
            };
            use tower_grpc::codegen::server::*;

            pub struct GetOrder<T>(pub T);

            impl<T> tower::Service for GetOrder<T> where T: EsiMarkets
            {
                type Request = grpc::Request<GetOrderRequest>;
                type Response = grpc::Response<GetOrdersResponse>;
                type Error = grpc::Error;
                type Future = T::GetOrderFuture;

                fn poll_ready(&mut self) -> futures::Poll<(), Self::Error> {
                    Ok(futures::Async::Ready(()))
                }

                fn call(&mut self, request: Self::Request) -> Self::Future {
                    self.0.get_order(request)
                }
            }

            pub struct GetRegion<T>(pub T);

            impl<T> tower::Service for GetRegion<T> where T: EsiMarkets
            {
                type Request = grpc::Request<GetRegionRequest>;
                type Response = grpc::Response<GetOrdersResponse>;
                type Error = grpc::Error;
                type Future = T::GetRegionFuture;

                fn poll_ready(&mut self) -> futures::Poll<(), Self::Error> {
                    Ok(futures::Async::Ready(()))
                }

                fn call(&mut self, request: Self::Request) -> Self::Future {
                    self.0.get_region(request)
                }
            }

            pub struct GetType<T>(pub T);

            impl<T> tower::Service for GetType<T> where T: EsiMarkets
            {
                type Request = grpc::Request<GetTypeRequest>;
                type Response = grpc::Response<GetOrdersResponse>;
                type Error = grpc::Error;
                type Future = T::GetTypeFuture;

                fn poll_ready(&mut self) -> futures::Poll<(), Self::Error> {
                    Ok(futures::Async::Ready(()))
                }

                fn call(&mut self, request: Self::Request) -> Self::Future {
                    self.0.get_type(request)
                }
            }

            pub struct GetRegionType<T>(pub T);

            impl<T> tower::Service for GetRegionType<T> where T: EsiMarkets
            {
                type Request = grpc::Request<GetRegionTypeRequest>;
                type Response = grpc::Response<GetOrdersResponse>;
                type Error = grpc::Error;
                type Future = T::GetRegionTypeFuture;

                fn poll_ready(&mut self) -> futures::Poll<(), Self::Error> {
                    Ok(futures::Async::Ready(()))
                }

                fn call(&mut self, request: Self::Request) -> Self::Future {
                    self.0.get_region_type(request)
                }
            }

            pub struct GetRegionTypeUpdateStream<T>(pub T);

            impl<T> tower::Service for GetRegionTypeUpdateStream<T> where T: EsiMarkets
            {
                type Request = grpc::Request<Empty>;
                type Response = grpc::Response<T::GetRegionTypeUpdateStreamStream>;
                type Error = grpc::Error;
                type Future = T::GetRegionTypeUpdateStreamFuture;

                fn poll_ready(&mut self) -> futures::Poll<(), Self::Error> {
                    Ok(futures::Async::Ready(()))
                }

                fn call(&mut self, request: Self::Request) -> Self::Future {
                    self.0.get_region_type_update_stream(request)
                }
            }
        }
    }
}
