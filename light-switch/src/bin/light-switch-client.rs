use light_switch::light_switch::{SwitchRequest, SwitchResponse, LightStatus};
use light_switch::{
    runtime_transport_config_path, setup_vsomeip_config, client_requested_service,
};
use std::sync::Arc;
use std::time::Instant;
use std::fs::File;
use std::io::Write;
use up_rust::communication::{CallOptions, InMemoryRpcClient, RpcClient, UPayload};
use up_rust::UPayloadFormat::UPAYLOAD_FORMAT_PROTOBUF_WRAPPED_IN_ANY;
use up_rust::UUri;
use up_transport_vsomeip::UPTransportVsomeip;
use prost::Message;

const CLIENT_CONFIG_FILE: &str = "client_sd.json";
const REQUEST_TTL_MS: u32 = 5000; // 5 seconds in milliseconds
const NUM_REQUESTS: usize = 10000;
const RTT_OUTPUT_FILE: &str = "rtt_measurements.csv";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    println!("[CLIENT] Light Switch Client starting");

    // 1) Setup vSomeIP config BEFORE initializing transport
    setup_vsomeip_config(CLIENT_CONFIG_FILE);
    let vsomeip_config = runtime_transport_config_path(CLIENT_CONFIG_FILE);
    println!("[CLIENT] vSomeIP config: {}", vsomeip_config.display());

    // 1b) Read client's requested service from CLIENT config (what we want to discover)
    let (service_id, methods) = client_requested_service(CLIENT_CONFIG_FILE)
        .ok_or("[CLIENT] unable to read requested services from client config")?;
    println!(
        "[CLIENT] requesting service 0x{:04X} with methods: {:?}",
        service_id,
        methods.iter().map(|m| format!("0x{:04X}", m)).collect::<Vec<_>>()
    );

    let first_method_id = methods.first().ok_or("[CLIENT] no methods configured")?;
    println!("[CLIENT] will use first method: 0x{:04X}", first_method_id);
    
    // DEBUG: verify resource_id calculation
    let debug_resource_id: u8 = (first_method_id & 0xFF) as u8;
    println!("[DEBUG] first_method_id={} (0x{:04X}), resource_id calculation: {} & 0xFF = {} (0x{:02X})", 
        first_method_id, first_method_id, first_method_id, debug_resource_id, debug_resource_id);
    
    if debug_resource_id == 0 {
        eprintln!("[ERROR] resource_id is 0! Method ID must be > 0");
        return Err("[ERROR] invalid method ID".into());
    }

    // 2) Create client UUri: app_id=0x4321, ue_id = 1
    let client_ue_id = 1u32;
    let client_uuri = UUri::try_from_parts("linux", client_ue_id, 0, 0)?;
    println!(
        "[CLIENT] client UUri: authority={}, ue_id=0x{:08x}",
        "linux", client_ue_id
    );

    // 3) Create client transport
    let client_transport = Arc::new(
        UPTransportVsomeip::new_with_config(
            client_uuri,
            &"linux".to_string(),
            &vsomeip_config,
            None,
        )?,
    );
    println!("[CLIENT] UPTransportVsomeip created");

    // 4) Create RPC client
    let l2_client = InMemoryRpcClient::new(client_transport.clone(), client_transport.clone())
        .await?;
    println!("[CLIENT] InMemoryRpcClient created");

    // 5) Build server sink UUri for RPC: use service_id as ue_id (instance discovered via SD)
    // Note: vSomeIP SD will discover the actual instance and route the request accordingly
    let server_ue_id = service_id as u32; // Just the service ID, not (instance << 16) | service

    // UUri.try_from_parts(authority, ue_id, major_version, method_id)
    let major_version: u8 = 0;  // third param: major version
    
    let server_sink = UUri::try_from_parts("linux", server_ue_id, major_version, *first_method_id)?;
    println!(
        "[CLIENT] server sink: service_id=0x{:04X}, method=0x{:04X}",
        service_id, first_method_id
    );
    println!("[CLIENT] NOTE: instance will be discovered via vSomeIP SD");

    // 6) Measurement loop
    let mut rtt_measurements = Vec::new();

    for i in 0..NUM_REQUESTS {
        //tokio::time::sleep(Duration::from_millis(500)).await;

        // Cycle through light statuses: OFF, SIDE_LIGHTS, LOW_BEAMS, HIGH_BEAMS
        let status_value = (i % 4) as i32;
        let status_enum = LightStatus::try_from(status_value).unwrap_or(LightStatus::Off);

        let switch_request = SwitchRequest {
            status: status_enum as i32,
        };

        //println!("[CLIENT] iteration {}: sending request with status={:?}", i, status_enum);

        // Encode request to UPayload
        let mut req_bytes = Vec::new();
        switch_request.encode(&mut req_bytes)?;
        let req_payload = UPayload::new(req_bytes, UPAYLOAD_FORMAT_PROTOBUF_WRAPPED_IN_ANY);

        // Measure RTT
        let start = Instant::now();
        let invoke_result = l2_client
            .invoke_method(
                server_sink.clone(),
                CallOptions::for_rpc_request(REQUEST_TTL_MS, None, None, None),
                Some(req_payload),
            )
            .await;
        let elapsed = start.elapsed();
        let rtt_ms = elapsed.as_secs_f64() * 1000.0;

        match invoke_result {
            Ok(response_opt) => {
                if let Some(resp_payload) = response_opt {
                    let protobuf_wrapped =
                        UPayload::new(resp_payload.payload(), UPAYLOAD_FORMAT_PROTOBUF_WRAPPED_IN_ANY);
                    match SwitchResponse::decode(&*protobuf_wrapped.payload()) {
                        Ok(switch_response) => {
                            println!(
                                "[CLIENT] iteration {}: RTT={:.3}ms, response status={}",
                                i, rtt_ms, switch_response.status
                            );
                            rtt_measurements.push((i, rtt_ms, "ok".to_string()));
                        }
                        Err(e) => {
                            eprintln!("[CLIENT] iteration {}: decode error: {}", i, e);
                            rtt_measurements.push((i, rtt_ms, format!("decode_error: {}", e)));
                        }
                    }
                } else {
                    eprintln!("[CLIENT] iteration {}: no response payload", i);
                    rtt_measurements.push((i, rtt_ms, "no_payload".to_string()));
                }
            }
            Err(e) => {
                eprintln!("[CLIENT] iteration {}: invoke error: {}", i, e);
                rtt_measurements.push((i, rtt_ms, format!("invoke_error: {}", e)));
            }
        }
    }

    // 7) Write RTT measurements to file
    write_rtt_file(RTT_OUTPUT_FILE, &rtt_measurements)?;

    Ok(())
}

fn write_rtt_file(filename: &str, measurements: &[(usize, f64, String)]) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = File::create(filename)?;

    // Write CSV header (milliseconds)
    writeln!(file, "iteration,rtt_ms,status")?;

    // Write measurements in milliseconds
    for (iter, rtt_ms, status) in measurements {
        writeln!(file, "{},{:.3},{}", iter, rtt_ms, status)?;
    }

    println!("[CLIENT] RTT measurements written to {}", filename);
    Ok(())
}