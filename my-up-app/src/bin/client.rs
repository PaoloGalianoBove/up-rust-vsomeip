use hello_world_protos::hello_world_service::{HelloRequest, HelloResponse};
use log::trace;
use std::sync::Arc;
use std::time::Duration;
use up_rust::communication::{CallOptions, InMemoryRpcClient, RpcClient, UPayload};
use up_rust::UPayloadFormat::UPAYLOAD_FORMAT_PROTOBUF_WRAPPED_IN_ANY;
use up_rust::UStatus;
use up_transport_vsomeip::UPTransportVsomeip;

use my_up_app::{
    client_transport_uuri, hello_service_sink_uuri, runtime_transport_config_path, setup_vsomeip_config, REQUEST_TTL,
};


#[tokio::main]
async fn main() -> Result<(), UStatus> {
    env_logger::init();

    println!("mE_client");
    
    // Setup vSomeIP config BEFORE initializing transport
    setup_vsomeip_config("client_sd.json");

    let vsomeip_config = runtime_transport_config_path("client_sd.json");
    trace!("vsomeip_config: {vsomeip_config:?}");

    let client = Arc::new(
        UPTransportVsomeip::new_with_config(
            client_transport_uuri(),
            &"linux".to_string(),
            &vsomeip_config,
            None,
        )
        .unwrap(),
    );

    let l2_client = InMemoryRpcClient::new(client.clone(), client.clone())
        .await
        .unwrap();

    let sink = hello_service_sink_uuri();
    // debug: print sink and client uuri details to help diagnose instance/service IDs
    println!(
        "client uuri: {:?}, sink ue_id=0x{:08x}, resource_id=0x{:04x}, ue_version={}",
        client_transport_uuri(),
        sink.ue_id,
        sink.resource_id,
        sink.ue_version_major
    );

    let mut iteration = 0;
    // Wait longer before the first request to allow SD to populate routing table
    println!("Waiting 2seconds for Service Discovery to populate routing table...");
    for remaining in (1..=2).rev() {
        if remaining % 5 == 0 {
            println!("  {} seconds remaining...", remaining);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    println!("SD wait complete. Starting method invocations...");
    
    loop {
        // regular pacing between attempts
        tokio::time::sleep(Duration::from_millis(1000)).await;

        let hello_request = HelloRequest {
            name: format!("me_client@i={iteration}"),
            ..Default::default()
        };
        iteration += 1;
        println!("Sending Request message with payload:\n{hello_request:?}");

        // retry loop: try up to 5 times before giving up
        let mut attempt = 0;
        let response = loop {
            attempt += 1;
            let call_options = CallOptions::for_rpc_request(REQUEST_TTL, None, None, None);
            let invoke_res = l2_client
                .invoke_method(
                    sink.clone(),
                    call_options,
                    Some(UPayload::try_from_protobuf(hello_request.clone()).unwrap()),
                )
                .await;

            if let Ok(ok) = invoke_res {
                break ok;
            }

            if attempt >= 5 {
                panic!(
                    "Hit an error attempting to invoke method after {} attempts: {:?}",
                    attempt,
                    invoke_res.err().unwrap()
                );
            }

            println!("Invoke attempt {} failed, retrying in 1s...", attempt);
            tokio::time::sleep(Duration::from_secs(1)).await;
        };

        let hello_response_vsomeip_unspecified_payload_format = response.unwrap();
        let hello_response_protobuf_payload_format = UPayload::new(
            hello_response_vsomeip_unspecified_payload_format.payload(),
            UPAYLOAD_FORMAT_PROTOBUF_WRAPPED_IN_ANY,
        );

        let Ok(hello_response) =
            hello_response_protobuf_payload_format.extract_protobuf::<HelloResponse>()
        else {
            panic!("Unable to parse into HelloResponse");
        };

        println!("Here we received response: {hello_response:?}");
    }
}