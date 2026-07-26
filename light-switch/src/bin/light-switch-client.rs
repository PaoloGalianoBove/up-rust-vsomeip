use light_switch::light_switch::{SwitchRequest, SwitchResponse, LightStatus};
use light_switch::{runtime_transport_config_path, setup_vsomeip_config, client_requested_service};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Instant, Duration};
use std::fs::File;
use std::io::Write;
use std::thread;
use sysinfo::{System, ProcessesToUpdate};
use up_rust::communication::{CallOptions, InMemoryRpcClient, RpcClient, UPayload};
use up_rust::UPayloadFormat::UPAYLOAD_FORMAT_PROTOBUF_WRAPPED_IN_ANY;
use up_rust::UUri;
use up_transport_vsomeip::UPTransportVsomeip;
use prost::Message;

#[derive(Debug, Clone, Copy, Default)]
struct ResourceSnapshot {
    proc_ram_mb: f64,
    proc_vsz_mb: f64,
    proc_cpu_pct: f32,
    sys_ram_pct: f64,
    sys_cpu_pct: f32,
}

struct AtomicResourceSnapshot {
    proc_ram_mb: AtomicU64,
    proc_vsz_mb: AtomicU64,
    proc_cpu_pct: AtomicU32,
    sys_ram_pct: AtomicU64,
    sys_cpu_pct: AtomicU32,
}

impl Default for AtomicResourceSnapshot {
    fn default() -> Self {
        Self {
            proc_ram_mb: AtomicU64::new(0f64.to_bits()),
            proc_vsz_mb: AtomicU64::new(0f64.to_bits()),
            proc_cpu_pct: AtomicU32::new(0f32.to_bits()),
            sys_ram_pct: AtomicU64::new(0f64.to_bits()),
            sys_cpu_pct: AtomicU32::new(0f32.to_bits()),
        }
    }
}

const CLIENT_CONFIG_FILE: &str = "client_sd.json";
const REQUEST_TTL_MS: u32 = 5000;
const RTT_OUTPUT_FILE: &str = "rtt_measurements.csv";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    
    // Spawn background resource monitoring thread
    let resource_snapshot = Arc::new(AtomicResourceSnapshot::default());
    let snapshot_clone = resource_snapshot.clone();

    thread::spawn(move || {
        let mut sys = System::new();
        let pid = sysinfo::get_current_pid().unwrap();
        
        sys.refresh_memory();
        let total_mem = sys.total_memory() as f64;
        let sys_ram_pct = if total_mem > 0.0 {
            (sys.used_memory() as f64 / total_mem) * 100.0
        } else {
            0.0
        };
        
        loop {
            sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), false);
            
            let mut proc_ram_mb = 0.0;
            let mut proc_vsz_mb = 0.0;
            let mut proc_cpu_pct = 0.0f32;
            
            if let Some(proc) = sys.process(pid) {
                proc_ram_mb = proc.memory() as f64 / (1024.0 * 1024.0);
                proc_vsz_mb = proc.virtual_memory() as f64 / (1024.0 * 1024.0);
                proc_cpu_pct = proc.cpu_usage();
            }
            
            snapshot_clone.proc_ram_mb.store(proc_ram_mb.to_bits(), Ordering::Relaxed);
            snapshot_clone.proc_vsz_mb.store(proc_vsz_mb.to_bits(), Ordering::Relaxed);
            snapshot_clone.proc_cpu_pct.store(proc_cpu_pct.to_bits(), Ordering::Relaxed);
            snapshot_clone.sys_ram_pct.store(sys_ram_pct.to_bits(), Ordering::Relaxed);
            snapshot_clone.sys_cpu_pct.store(0.0f32.to_bits(), Ordering::Relaxed);
            
            thread::sleep(Duration::from_millis(200));
        }
    });

    setup_vsomeip_config(CLIENT_CONFIG_FILE);
    let vsomeip_config = runtime_transport_config_path(CLIENT_CONFIG_FILE);

    let (service_id, methods) = client_requested_service(CLIENT_CONFIG_FILE)
        .ok_or("[CLIENT] Unable to read config")?;
    let first_method_id = methods.first().ok_or("[CLIENT] No methods configured")?;

    let client_uuri = UUri::try_from_parts("linux", 1, 0, 0)?;
    let client_transport = Arc::new(
        UPTransportVsomeip::new_with_config(
            client_uuri,
            &"linux".to_string(),
            &vsomeip_config,
            None,
        )?,
    );

    let l2_client = InMemoryRpcClient::new(client_transport.clone(), client_transport.clone()).await?;
    let server_sink = UUri::try_from_parts("linux", service_id as u32, 0, *first_method_id)?;

    println!("[CLIENT] Stabilizing connection...");
    tokio::time::sleep(Duration::from_secs(2)).await;

    let mut rtt_measurements = Vec::new();

    for i in 0..10 {
        let snapshot = ResourceSnapshot {
            proc_ram_mb: f64::from_bits(resource_snapshot.proc_ram_mb.load(Ordering::Relaxed)),
            proc_vsz_mb: f64::from_bits(resource_snapshot.proc_vsz_mb.load(Ordering::Relaxed)),
            proc_cpu_pct: f32::from_bits(resource_snapshot.proc_cpu_pct.load(Ordering::Relaxed)),
            sys_ram_pct: f64::from_bits(resource_snapshot.sys_ram_pct.load(Ordering::Relaxed)),
            sys_cpu_pct: f32::from_bits(resource_snapshot.sys_cpu_pct.load(Ordering::Relaxed)),
        };

        let status_val = (i % 4) as i32;
        let switch_request = SwitchRequest { status: status_val };

        let mut req_bytes = Vec::new();
        switch_request.encode(&mut req_bytes)?;

        let req_payload = UPayload::new(req_bytes, UPAYLOAD_FORMAT_PROTOBUF_WRAPPED_IN_ANY);
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
                    let raw_bytes = resp_payload.payload();
                    let protobuf_wrapped = UPayload::new(raw_bytes, UPAYLOAD_FORMAT_PROTOBUF_WRAPPED_IN_ANY);
                    match SwitchResponse::decode(&*protobuf_wrapped.payload()) {
                        Ok(switch_response) => {
                            let status_enum = LightStatus::try_from(switch_response.status).unwrap_or(LightStatus::Off);
                            println!("[CLIENT] Iteration {} success: RTT = {:.2}ms, Status = {:?}", i, rtt_ms, status_enum);
                            rtt_measurements.push((i, rtt_ms, "ok".to_string(), snapshot));
                        }
                        Err(e) => {
                            eprintln!("[CLIENT] Iteration {} decode error: {}", i, e);
                            rtt_measurements.push((i, rtt_ms, format!("decode_error: {}", e), snapshot));
                        }
                    }
                } else {
                    eprintln!("[CLIENT] Iteration {} empty payload error", i);
                    rtt_measurements.push((i, rtt_ms, "no_payload".to_string(), snapshot));
                }
            }
            Err(e) => {
                println!("[CLIENT] Iteration {} invocation error: {:?}", i, e);
                rtt_measurements.push((i, rtt_ms, format!("invoke_error: {:?}", e), snapshot));
            }
        }
    }
    
    write_rtt_file(RTT_OUTPUT_FILE, &rtt_measurements)?;
    println!("[CLIENT] Completed.");
    Ok(())
}

fn write_rtt_file(filename: &str, measurements: &[(usize, f64, String, ResourceSnapshot)]) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = File::create(filename)?;
    writeln!(file, "iteration,rtt_ms,status,proc_ram_mb,proc_vsz_mb,proc_cpu_pct,sys_ram_pct,sys_cpu_pct")?;

    for (iter, rtt_ms, status, snapshot) in measurements {
        writeln!(
            file,
            "{},{:.3},{},{:.3},{:.3},{:.1},{:.1},{:.1}",
            iter,
            rtt_ms,
            status,
            snapshot.proc_ram_mb,
            snapshot.proc_vsz_mb,
            snapshot.proc_cpu_pct,
            snapshot.sys_ram_pct,
            snapshot.sys_cpu_pct
        )?;
    }

    println!("[CLIENT] RTT measurements written to {}", filename);
    Ok(())
}