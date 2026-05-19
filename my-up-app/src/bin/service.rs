use async_trait::async_trait;
use hello_world_protos::hello_world_service::{HelloRequest, HelloResponse};
use log::{error, trace};
use std::sync::Arc;
use std::thread;
use up_rust::communication::{InMemoryRpcServer, RequestHandler, RpcServer, ServiceInvocationError, UPayload};
use up_rust::UPayloadFormat::UPAYLOAD_FORMAT_PROTOBUF_WRAPPED_IN_ANY;
use up_rust::{UAttributes, UCode, UStatus};
use up_transport_vsomeip::UPTransportVsomeip;

use my_up_app::{
    runtime_transport_config_path, service_transport_uuri, setup_vsomeip_config, CLIENT_AUTHORITY, HELLO_METHOD_ID,
};

struct ServiceRequestHandler;

impl ServiceRequestHandler {
    fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RequestHandler for ServiceRequestHandler {
    async fn handle_request(
        &self,
        resource_id: u16,
        _uattributes: &UAttributes,
        request_payload: Option<UPayload>,
    ) -> Result<Option<UPayload>, ServiceInvocationError> {
        println!(
            "ServiceRequestHandler: Received a resource_id: {resource_id} request_payload: {request_payload:?}"
        );

        let hello_request_vsomeip_unspecified_payload_format = request_payload.unwrap();
        let hello_request_protobuf_payload_format = UPayload::new(
            hello_request_vsomeip_unspecified_payload_format.payload(),
            UPAYLOAD_FORMAT_PROTOBUF_WRAPPED_IN_ANY,
        );
        let hello_request = hello_request_protobuf_payload_format.extract_protobuf::<HelloRequest>();

        let hello_request = match hello_request {
            Ok(hello_request) => {
                println!("hello_request: {hello_request:?}");
                hello_request
            }
            Err(err) => {
                error!("Unable to parse HelloRequest: {err:?}");
                return Err(ServiceInvocationError::RpcError(UStatus::fail_with_code(
                    UCode::INTERNAL,
                    "Unable to parse hello_request",
                )));
            }
        };

        let hello_response = HelloResponse {
            message: format!("The response to the request: {}", hello_request.name),
            ..Default::default()
        };

        println!("Making response to send back: {hello_response:?}");

        Ok(Some(UPayload::try_from_protobuf(hello_response).unwrap()))
    }
}

#[tokio::main]
async fn main() -> Result<(), UStatus> {
    env_logger::init();

    println!("mE_service");
    
    // Setup vSomeIP config BEFORE initializing transport
    setup_vsomeip_config("service_sd.json");

    let vsomeip_config = runtime_transport_config_path("service_sd.json");
    trace!("vsomeip_config: {vsomeip_config:?}");

    let service = Arc::new(
        UPTransportVsomeip::new_with_config(
            service_transport_uuri(),
            &CLIENT_AUTHORITY.to_string(),
            &vsomeip_config,
            None,
        )
        .unwrap(),
    );
    let l2_service = InMemoryRpcServer::new(service.clone(), service.clone());

    let service_request_handler = Arc::new(ServiceRequestHandler::new());
    l2_service
        .register_endpoint(None, HELLO_METHOD_ID, service_request_handler)
        .await
        .expect("Unable to register endpoint");

    thread::park();
    Ok(())
}