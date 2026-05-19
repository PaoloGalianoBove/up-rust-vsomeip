use std::fs;
use std::process::Command;
use std::net::UdpSocket;
use std::path::PathBuf;
use std::sync::Once;
use std::thread;
use std::time::Duration;

use up_rust::UUri;

static SETUP_ONCE: Once = Once::new();

pub const HELLO_SERVICE_ID: u16 = 0x6000;
pub const HELLO_INSTANCE_ID: u32 = 0x0001;
pub const HELLO_METHOD_ID: u16 = 0x7FFF;
pub const HELLO_SERVICE_MAJOR: u8 = 1;
pub const HELLO_SERVICE_AUTHORITY: &str = "linux";
pub const CLIENT_AUTHORITY: &str = "me_authority";
pub const CLIENT_UE_ID: u32 = 0x4321;
pub const CLIENT_UE_VERSION_MAJOR: u8 = 1;
pub const CLIENT_RESOURCE_ID: u16 = 0;
pub const REQUEST_TTL: u32 = 1000;

pub fn transport_config_path(file_name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("vsomeip_configs")
        .join(file_name)
}

fn detect_local_ipv4() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    // This does not send packets, but lets the kernel pick the outgoing interface.
    socket.connect("1.1.1.1:80").ok()?;
    let local = socket.local_addr().ok()?;
    let ip = local.ip().to_string();
    if ip != "127.0.0.1" {
        return Some(ip);
    }

    // Fallback when socket route resolution points to loopback.
    let output = Command::new("hostname").arg("-i").output().ok()?;
    let stdout = String::from_utf8(output.stdout).ok()?;
    stdout
        .split_whitespace()
        .find(|candidate| {
            candidate
                .split('.')
                .all(|part| part.parse::<u8>().is_ok())
                && *candidate != "127.0.0.1"
        })
        .map(|s| s.to_string())
}

fn detect_local_interface() -> Option<String> {
    let output = Command::new("sh")
        .arg("-c")
        .arg("ip route get 1.1.1.1 2>/dev/null | awk '{for (i=1; i<=NF; i++) if ($i==\"dev\") {print $(i+1); exit}}'")
        .output()
        .ok()?;

    let stdout = String::from_utf8(output.stdout).ok()?;
    let iface = stdout.trim();
    if iface.is_empty() {
        None
    } else {
        Some(iface.to_string())
    }
}

fn detect_netmask(ip: &str) -> &'static str {
    // Docker default bridge is typically 172.17.0.0/16.
    if ip.starts_with("172.17.") {
        "255.255.0.0"
    } else {
        "255.255.255.0"
    }
}

fn ensure_multicast_route() {
    let iface = detect_local_interface().unwrap_or_else(|| "eth0".to_string());
    let cmd = format!(
        "ip route show 224.0.0.0/4 | grep -q 'dev {iface}' || sudo ip route add 224.0.0.0/4 dev {iface}"
    );
    match Command::new("sh").arg("-c").arg(cmd).status() {
        Ok(status) if status.success() => {
            println!("runtime vSomeIP config: multicast route 224.0.0.0/4 via {iface} ready");
        }
        Ok(status) => {
            println!(
                "runtime vSomeIP config: unable to ensure multicast route, exit status: {status}"
            );
        }
        Err(err) => {
            println!("runtime vSomeIP config: unable to run route command: {err}");
        }
    }
}

pub fn runtime_transport_config_path(file_name: &str) -> PathBuf {
    let source_path = transport_config_path(file_name);
    let Ok(mut content) = fs::read_to_string(&source_path) else {
        return source_path;
    };

    if file_name.ends_with("_sd.json") {
        ensure_multicast_route();
    }

    if let Some(ip) = detect_local_ipv4() {
        content = content.replace("\"unicast\": \"0.0.0.0\"", &format!("\"unicast\": \"{ip}\""));
        let netmask = detect_netmask(&ip);
        content = content.replace("\"netmask\": \"255.255.255.0\"", &format!("\"netmask\": \"{netmask}\""));
        println!("runtime vSomeIP config: unicast={ip} netmask={netmask} file={file_name}");
    }

    if let Some(iface) = detect_local_interface() {
        content = content.replace("\"device\": \"eth0\"", &format!("\"device\": \"{iface}\""));
        println!("runtime vSomeIP config: device={iface} file={file_name}");
    }

    let runtime_path = std::env::temp_dir().join(format!("vsomeip-{}-{}.json", file_name, std::process::id()));
    if fs::write(&runtime_path, content).is_ok() {
        runtime_path
    } else {
        source_path
    }
}

pub fn client_transport_uuri() -> UUri {
    UUri::try_from_parts(
        CLIENT_AUTHORITY,
        CLIENT_UE_ID,
        CLIENT_UE_VERSION_MAJOR,
        CLIENT_RESOURCE_ID,
    )
    .unwrap()
}

pub fn service_transport_uuri() -> UUri {
    let ue_id = (HELLO_INSTANCE_ID << 16) | HELLO_SERVICE_ID as u32;
    UUri::try_from_parts(HELLO_SERVICE_AUTHORITY, ue_id, HELLO_SERVICE_MAJOR, 0).unwrap()
}

pub fn hello_service_sink_uuri() -> UUri {
    let ue_id = (HELLO_INSTANCE_ID << 16) | HELLO_SERVICE_ID as u32;
    UUri::try_from_parts(
        HELLO_SERVICE_AUTHORITY,
        ue_id,
        HELLO_SERVICE_MAJOR,
        HELLO_METHOD_ID,
    )
    .unwrap()
}

/// Setup vSomeIP configuration before initializing transport.
/// Must be called once before UPTransportVsomeip::new_with_config.
pub fn setup_vsomeip_config(sd_file_name: &str) {
    SETUP_ONCE.call_once(|| {
        // Generate runtime config and set VSOMEIP_CONFIGURATION env var
        let runtime_path = runtime_transport_config_path(sd_file_name);
        std::env::set_var("VSOMEIP_CONFIGURATION", runtime_path.to_string_lossy().to_string());
        println!("setup_vsomeip_config: VSOMEIP_CONFIGURATION set to {}", runtime_path.display());
        // Give the env var time to propagate to native libraries
        thread::sleep(Duration::from_millis(50));
    });
}