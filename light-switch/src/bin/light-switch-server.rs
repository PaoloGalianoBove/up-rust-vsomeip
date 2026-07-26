use light_switch::{
    get_server_identifiers, init_server_identifiers, runtime_transport_config_path,
    setup_vsomeip_config, light_switch::{SwitchRequest, SwitchResponse, LightStatus},
};
use up_rust::communication::{RequestHandler, RpcServer, UPayload, ServiceInvocationError};
use up_rust::{UAttributes, UCode, UStatus};
use up_rust::UPayloadFormat::UPAYLOAD_FORMAT_PROTOBUF_WRAPPED_IN_ANY;
use prost::Message;
use std::sync::atomic::{AtomicUsize, Ordering};

const SERVER_CONFIG_FILE: &str = "service_sd.json";

struct SwitchHandler {
    request_counter: AtomicUsize,
}

#[async_trait::async_trait]
impl RequestHandler for SwitchHandler {
    async fn handle_request(
        &self,
        method_id: u16,
        _attributes: &UAttributes,
        request_payload: Option<UPayload>,
    ) -> Result<Option<UPayload>, ServiceInvocationError> {
        let req_idx = self.request_counter.fetch_add(1, Ordering::Relaxed);
        
        let vsomeip_unspecified = match request_payload {
            Some(p) => p,
            None => {
                eprintln!("[SERVER] Error: empty request payload");
                return Err(ServiceInvocationError::RpcError(UStatus::fail_with_code(
                    UCode::INTERNAL,
                    "empty payload",
                )));
            }
        };

        let raw_request_bytes = vsomeip_unspecified.payload();
        let protobuf_payload = UPayload::new(raw_request_bytes, UPAYLOAD_FORMAT_PROTOBUF_WRAPPED_IN_ANY);
        
        let switch_req = match SwitchRequest::decode(&*protobuf_payload.payload()) {
            Ok(r) => r,
            Err(err) => {
                eprintln!("[SERVER] Error: failed to parse request: {:?}", err);
                return Err(ServiceInvocationError::RpcError(UStatus::fail_with_code(
                    UCode::INTERNAL,
                    "parse error",
                )));
            }
        };

        let status_enum = LightStatus::try_from(switch_req.status).unwrap_or(LightStatus::Off);
        println!("[SERVER] Received Request #{}: status = {:?}", req_idx, status_enum);

        let switch_response = SwitchResponse { status: switch_req.status };
        let mut resp_bytes = Vec::new();
        switch_response.encode(&mut resp_bytes).map_err(|_| {
            ServiceInvocationError::RpcError(UStatus::fail_with_code(UCode::INTERNAL, "encode error"))
        })?;

        let resp_payload = UPayload::new(resp_bytes, UPAYLOAD_FORMAT_PROTOBUF_WRAPPED_IN_ANY);
        Ok(Some(resp_payload))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    println!("[SERVER] Starting Light Switch Server...");

    setup_vsomeip_config(SERVER_CONFIG_FILE);
    init_server_identifiers(SERVER_CONFIG_FILE).ok_or("[SERVER] Unable to read server config")?;
    let ids = get_server_identifiers().unwrap();
    let runtime_config = runtime_transport_config_path(SERVER_CONFIG_FILE);

    let server_uuri = up_rust::UUri::try_from_parts("linux", ids.service_id as u32, 0, 0)?;
    let transport = up_transport_vsomeip::UPTransportVsomeip::new_with_config(
        server_uuri,
        &"linux".to_string(),
        &runtime_config,
        None,
    )?;

    let transport = std::sync::Arc::new(transport);
    let l2_service = up_rust::communication::InMemoryRpcServer::new(transport.clone(), transport.clone());

    let switch_handler = std::sync::Arc::new(SwitchHandler {
        request_counter: AtomicUsize::new(0),
    });
    
    let methods = light_switch::server_methods();
    for method_id in methods {
        l2_service.register_endpoint(None, method_id, switch_handler.clone()).await?;
    }

    println!("[SERVER] Server is ready and waiting for requests...");
    std::thread::park();
    Ok(())
}
