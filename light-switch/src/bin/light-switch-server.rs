use light_switch::{
    get_server_identifiers, init_server_identifiers, runtime_transport_config_path,
    setup_vsomeip_config, transport_config_path, light_switch::{SwitchRequest, SwitchResponse},
};
use up_rust::communication::{RequestHandler, RpcServer, UPayload, ServiceInvocationError};
use up_rust::{UAttributes, UCode, UStatus};
use up_rust::UPayloadFormat::UPAYLOAD_FORMAT_PROTOBUF_WRAPPED_IN_ANY;
use prost::Message;

const SERVER_CONFIG_FILE: &str = "service_sd.json";

/// Handler for the LightSwitchService::Switch RPC method.
/// Implements RequestHandler trait to handle incoming requests.
struct SwitchHandler;

#[async_trait::async_trait]
impl RequestHandler for SwitchHandler {
    async fn handle_request(
        &self,
        method_id: u16,
        attributes: &UAttributes,
        request_payload: Option<UPayload>,
    ) -> Result<Option<UPayload>, ServiceInvocationError> {
        eprintln!("[SWITCH_HANDLER] received request for method 0x{:04X}", method_id);
        eprintln!("[SWITCH_HANDLER] request attributes: {:?}", attributes);

        // Follow the structure used in my-up-app ServiceRequestHandler:
        // - unwrap the incoming UPayload
        // - wrap it as protobuf format UPayload
        // - decode with prost from inner bytes (SwitchRequest is prost-generated)
        let vsomeip_unspecified = match request_payload {
            Some(p) => p,
            None => {
                eprintln!("[SWITCH_HANDLER] no payload in request");
                return Err(ServiceInvocationError::RpcError(UStatus::fail_with_code(
                    UCode::INTERNAL,
                    "empty payload",
                )));
            }
        };

        let raw_request_bytes = vsomeip_unspecified.payload();
        eprintln!(
            "[SWITCH_HANDLER] raw request payload size: {} bytes",
            raw_request_bytes.len()
        );

        //Definition of the payload into a proper upayload with the correct format to be able to extract the protobuf message
        let protobuf_payload = UPayload::new(raw_request_bytes, UPAYLOAD_FORMAT_PROTOBUF_WRAPPED_IN_ANY);

        // Decode the protobuf message in order to get the SwitchRequest struct. This is where we use the prost-generated struct and its decode method.
        let switch_req = match SwitchRequest::decode(&*protobuf_payload.payload()) {
            Ok(r) => r,
            Err(err) => {
                eprintln!("[SWITCH_HANDLER] Unable to parse SwitchRequest: {err:?}");
                return Err(ServiceInvocationError::RpcError(UStatus::fail_with_code(
                    UCode::INTERNAL,
                    "Unable to parse SwitchRequest",
                )));
            }
        };

        eprintln!("[SWITCH_HANDLER] parsed request status: {}", switch_req.status);

        // Business logic to generate response based on request; here we simply echo the status back in the response.
        let switch_response = SwitchResponse { status: switch_req.status };
        // Encode response and wrap as UPayload (avoid try_from_protobuf)
        let mut resp_bytes = Vec::new();
        if let Err(e) = switch_response.encode(&mut resp_bytes) {
            eprintln!("[SWITCH_HANDLER] failed to encode SwitchResponse: {e}");
            return Err(ServiceInvocationError::RpcError(UStatus::fail_with_code(
                UCode::INTERNAL,
                "response encode error",
            )));
        }

        eprintln!(
            "[SWITCH_HANDLER] response payload size: {} bytes, status: {}",
            resp_bytes.len(),
            switch_response.status
        );
        eprintln!(
            "[SWITCH_HANDLER] sending response for method 0x{:04X}",
            method_id
        );

        let resp_payload = UPayload::new(resp_bytes, UPAYLOAD_FORMAT_PROTOBUF_WRAPPED_IN_ANY);
        Ok(Some(resp_payload))
    }
}

// helper removed: runtime checks now parse config values directly

fn verify_server_config_alignment(config_file: &str) -> Result<(), String> {
    println!(
        "[CHECK] using config file: {}",
        transport_config_path(config_file).display()
    );

    // Initialize once (idempotent)
    init_server_identifiers(config_file)
        .ok_or_else(|| "[FAIL] unable to parse server identifiers from config".to_string())?;

    // Read back from the one-time storage
    let ids = get_server_identifiers().unwrap();
    println!("[PASS] service id from config: 0x{:04X}", ids.service_id);
    println!("[PASS] instance id from config: 0x{:04X}", ids.instance_id);
    println!("[PASS] server app id from config: {}", ids.app_id);
    println!("[PASS] server port from config: {}", ids.port);

    println!("[INFO] client app id reserved for client bootstrap: <client-app-id>");

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    println!("Light Switch Server bootstrap");

    // 1) Static checks: constants vs source JSON config.
    verify_server_config_alignment(SERVER_CONFIG_FILE)?;

    // 2) Setup process environment for vSomeIP.
    setup_vsomeip_config(SERVER_CONFIG_FILE);

    // 3) Runtime config generation check.
    let runtime_config = runtime_transport_config_path(SERVER_CONFIG_FILE);
    println!("[PASS] runtime config path: {}", runtime_config.display());

    println!("[NEXT] checks done. Creating UPTransportVsomeip...");

    // Build transport identity (UUri) from config identifiers
    // Ensure identifiers initialized once and read from storage
    init_server_identifiers(SERVER_CONFIG_FILE).ok_or("unable to read identifiers from config")?;
    let ids = get_server_identifiers().unwrap();
    let service_id = ids.service_id;
    let _instance_id = ids.instance_id;

    // Use only service_id as ue_id (must be < 0x8000 for RPC)
    // Instance is discovered and routed by vSomeIP SD, not encoded in ue_id
    let ue_id = service_id as u32;
    let authority = "linux"; // change if you need a different authority

    // try_from_parts(authority, ue_id, major_version, method_id)
    // Use method_id=0 for the service generic endpoint
    let uuri = up_rust::UUri::try_from_parts(authority, ue_id, 0, 0)?;

    // Instantiate UPTransportVsomeip
    let transport = up_transport_vsomeip::UPTransportVsomeip::new_with_config(
        uuri,
        &authority.to_string(),
        &runtime_config,
        None,
    )?;

    let transport = std::sync::Arc::new(transport);
    println!("[PASS] UPTransportVsomeip created and wrapped in Arc");

    // Create L2 RPC server wrapper (in-memory) for registering endpoints later
    let l2_service =
        up_rust::communication::InMemoryRpcServer::new(transport.clone(), transport.clone());
    println!("[PASS] InMemoryRpcServer created. Ready to register endpoints.");

    // Log configured methods (populated from JSON `services[0].methods`).
    let methods = light_switch::server_methods();
    if methods.is_empty() {
        println!("[WARN] no methods configured for service; nothing to register yet");
    } else {
        println!(
            "[INFO] configured method ids: {:?}",
            methods
                .iter()
                .map(|m| format!("0x{:04X}", m))
                .collect::<Vec<_>>()
        );
    }

    // Register the SwitchHandler for each configured method
    let switch_handler = std::sync::Arc::new(SwitchHandler);
    for method_id in methods {
        match l2_service.register_endpoint(None, method_id, switch_handler.clone()).await {
            Ok(_) => println!("[PASS] registered SwitchHandler for method 0x{:04X}", method_id),
            Err(e) => {
                eprintln!("[FAIL] failed to register endpoint for method 0x{:04X}: {}", method_id, e);
                return Err(format!("registration failed: {}", e).into());
            }
        }
    }

    

    println!("[PASS] all endpoints registered. server is ready to handle requests.");

    // Keep process alive for manual testing; endpoint registration is next step.
    std::thread::park();
    // unreachable
    // Ok is not reached because thread is parked; return Ok to satisfy signature
    // (in practice this code is never executed)
    // but keep for type checking
    // Note: if you want graceful shutdown, replace park() with a signal-aware loop.
    Ok(())
}
