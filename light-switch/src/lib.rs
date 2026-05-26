use std::fs;
use std::net::UdpSocket;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;
use std::thread;
use std::time::Duration;

static SETUP_ONCE: Once = Once::new();

pub mod light_switch {
    include!(concat!(env!("OUT_DIR"), "/light_switch.rs"));
}

/// Costruisce il percorso assoluto di un file JSON di configurazione vSomeIP.
///
/// La funzione parte dalla root del crate corrente (`CARGO_MANIFEST_DIR`) e
/// appende la cartella `vsomeip_configs` più il nome file passato dal chiamante.
///
/// Esempio: `transport_config_path("service_sd.json")` produce un percorso come
/// `<crate>/vsomeip_configs/service_sd.json`.
pub fn transport_config_path(file_name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("vsomeip_configs")
        .join(file_name)
}

/// Prova a rilevare l'indirizzo IPv4 locale non-loopback usato per uscire in rete.
///
/// Crea una socket UDP temporanea e fa `connect` verso `1.1.1.1:80`: non invia
/// traffico applicativo, ma forza il kernel a scegliere l'interfaccia/indirizzo
/// sorgente. Se l'indirizzo risultante e' `127.0.0.1`, ritorna `None`.
fn detect_local_ipv4() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    // No packet is sent; the kernel still selects the outbound interface for us.
    socket.connect("1.1.1.1:80").ok()?;
    let local = socket.local_addr().ok()?;
    let ip = local.ip().to_string();
    if ip != "127.0.0.1" {
        Some(ip)
    } else {
        None
    }
}

/// Rileva il nome dell'interfaccia di rete usata per il routing esterno.
///
/// Esegue `ip route get 1.1.1.1` e ne estrae il campo `dev` tramite `awk`.
/// Restituisce `Some("eth0")` (o equivalente) quando disponibile, altrimenti
/// `None` se il comando fallisce o non produce output valido.
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

/// Determina una netmask plausibile partendo dall'IP locale rilevato.
///
/// Al momento gestisce in modo esplicito la rete Docker bridge tipica
/// `172.17.x.x` come `/16` (`255.255.0.0`) e usa `/24`
/// (`255.255.255.0`) come fallback generale.
fn detect_netmask(ip: &str) -> &'static str {
    if ip.starts_with("172.17.") {
        "255.255.0.0"
    } else {
        "255.255.255.0"
    }
}

/// Garantisce la presenza della route multicast `224.0.0.0/4` sull'interfaccia attiva.
///
/// Questa route e' utile quando usi Service Discovery (SD), perche' vSomeIP fa
/// affidamento sul traffico multicast. Se l'interfaccia non e' rilevabile,
/// usa `eth0` come fallback.
fn ensure_multicast_route() {
    // In container/dev environments we often do not have CAP_NET_ADMIN,
    // so trying to add the route would only emit noisy RTNETLINK warnings.
    // Keep this as a no-op unless route management is explicitly reintroduced.
    let _ = detect_local_interface();
}

/// Genera un file di configurazione vSomeIP "runtime" adattato all'ambiente corrente.
///
/// Flusso:
/// 1. Legge il JSON sorgente da `vsomeip_configs/<file_name>`.
/// 2. Se e' un file `_sd.json`, prova a preparare la route multicast.
/// 3. Sostituisce nel JSON i placeholder principali (`unicast`, `netmask`, `device`)
///    con valori rilevati a runtime.
/// 4. Scrive il JSON risultante in `/tmp` con nome univoco per processo.
///
/// In caso di errore lettura/scrittura, ritorna il percorso del file sorgente.
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
    }

    if let Some(iface) = detect_local_interface() {
        content = content.replace("\"device\": \"eth0\"", &format!("\"device\": \"{iface}\""));
    }

    let runtime_path = std::env::temp_dir().join(format!(
        "vsomeip-{}-{}.json",
        file_name,
        std::process::id()
    ));
    // Prefer to insert a conservative logging section to limit native vsomeip
    // verbosity in dev/CI environments. Try JSON parse first, then fall back
    // to a textual injection, and finally write raw content as last resort.
    if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&content) {
        let logging = serde_json::json!({
            "console": { "level": "error" },
            "file": { "level": "off" }
        });
        v["logging"] = logging;
        if let Ok(s) = serde_json::to_string_pretty(&v) {
            if fs::write(&runtime_path, s).is_ok() {
                return runtime_path;
            }
        }
    }

    // Textual fallback: insert logging block before the final '}' if possible.
    if let Some(pos) = content.rfind('}') {
        let mut injected = String::new();
        injected.push_str(&content[..pos]);
        injected.push_str(",\n  \"logging\": {\n    \"console\": { \"level\": \"error\" },\n    \"file\": { \"level\": \"off\" }\n  }\n}");
        if fs::write(&runtime_path, injected).is_ok() {
            return runtime_path;
        }
    }

    // Final fallback: write raw content.
    if fs::write(&runtime_path, content).is_ok() {
        runtime_path
    } else {
        source_path
    }
}

/// Load the JSON config as serde_json::Value from `vsomeip_configs/<file_name>`.
pub fn load_config_value(file_name: &str) -> Option<serde_json::Value> {
    let path = transport_config_path(file_name);
    let content = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

fn parse_hex_u32(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x") {
        u32::from_str_radix(hex, 16).ok()
    } else {
        s.parse::<u32>().ok()
    }
}

fn parse_hex_u16(s: &str) -> Option<u16> {
    parse_hex_u32(s).and_then(|v| u16::try_from(v).ok())
}

/// Read server/service identifiers from the given config file.
/// Returns (service_id, instance_id, app_id, port) if available.
pub fn server_identifiers_from_config(file_name: &str) -> Option<(u16, u32, String, u16)> {
    let v = load_config_value(file_name)?;
    let service_str = v
        .get("services")?
        .get(0)?
        .get("service")?
        .as_str()?;
    let instance_str = v
        .get("services")?
        .get(0)?
        .get("instance")?
        .as_str()?;
    let app_id = v.get("applications")?.get(0)?.get("id")?.as_str()?.to_string();
    let port = v.get("port")?.as_u64()? as u16;

    let service_id = parse_hex_u16(service_str)?;
    let instance_id = parse_hex_u32(instance_str)?;

    Some((service_id, instance_id, app_id, port))
}

use once_cell::sync::OnceCell;

/// Struct holding server identifiers populated from config.
pub struct ServerIdentifiers {
    pub service_id: u16,
    pub instance_id: u32,
    pub app_id: String,
    pub port: u16,
    pub methods: Vec<u16>,
}

static SERVER_IDENTIFIERS: OnceCell<ServerIdentifiers> = OnceCell::new();

/// Initialize server identifiers once from the given JSON config file.
/// Returns a reference to the stored identifiers on success.
pub fn init_server_identifiers(file_name: &str) -> Option<&'static ServerIdentifiers> {
    if let Some(existing) = SERVER_IDENTIFIERS.get() {
        return Some(existing);
    }

    let ids = server_identifiers_from_config(file_name)?;
    let (service_id, instance_id, app_id, port) = ids;

    // parse methods array inside services[0].methods if present
    let methods: Vec<u16> = load_config_value(file_name)
        .and_then(|cfg| {
            cfg.get("services")
                .and_then(|s| s.get(0))
                .and_then(|s0| s0.get("methods"))
                .and_then(|m| m.as_array().map(|arr| arr.to_owned()))
        })
        .map(|arr| {
            arr.into_iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .filter_map(|s| parse_hex_u16(&s))
                .collect()
        })
        // fallback to top-level legacy `method` string, if present
        .or_else(|| {
            load_config_value(file_name).and_then(|cfg| {
                cfg.get("method")
                    .and_then(|m| m.as_str())
                    .and_then(|s| parse_hex_u16(s).map(|v| vec![v]))
            })
        })
        .unwrap_or_else(|| Vec::new());

    let si = ServerIdentifiers {
        service_id,
        instance_id,
        app_id,
        port,
        methods,
    };

    // Attempt to set; ignore if another thread set it first.
    let _ = SERVER_IDENTIFIERS.set(si);
    SERVER_IDENTIFIERS.get()
}

/// Get the initialized server identifiers if available.
pub fn get_server_identifiers() -> Option<&'static ServerIdentifiers> {
    SERVER_IDENTIFIERS.get()
}

/// Convenience getters that fall back to compile-time consts when not initialized.
pub fn svc_id() -> u16 {
    match get_server_identifiers() {
        Some(s) => s.service_id,
        None => {
            eprintln!("[FATAL] server identifiers not initialized: call init_server_identifiers(<config.json>) before using transport");
            std::process::exit(1);
        }
    }
}

pub fn instance_id() -> u32 {
    match get_server_identifiers() {
        Some(s) => s.instance_id,
        None => {
            eprintln!("[FATAL] server identifiers not initialized: call init_server_identifiers(<config.json>) before using transport");
            std::process::exit(1);
        }
    }
}

pub fn server_app_id() -> String {
    match get_server_identifiers() {
        Some(s) => s.app_id.clone(),
        None => {
            eprintln!("[FATAL] server identifiers not initialized: call init_server_identifiers(<config.json>) before using transport");
            std::process::exit(1);
        }
    }
}

pub fn server_port() -> u16 {
    match get_server_identifiers() {
        Some(s) => s.port,
        None => {
            eprintln!("[FATAL] server identifiers not initialized: call init_server_identifiers(<config.json>) before using transport");
            std::process::exit(1);
        }
    }
}

pub fn server_method_id() -> u16 {
    match get_server_identifiers() {
        Some(s) => s.methods.get(0).copied().unwrap_or_else(|| {
            eprintln!("[FATAL] no methods configured for service: configure at least one method in vsomeip_configs");
            std::process::exit(1);
        }),
        None => {
            eprintln!("[FATAL] server identifiers not initialized: call init_server_identifiers(<config.json>) before using transport");
            std::process::exit(1);
        }
    }
}

/// Return the configured server methods as a Vec<u16>.
/// If no methods are configured, returns an empty Vec.
pub fn server_methods() -> Vec<u16> {
    match get_server_identifiers() {
        Some(s) => s.methods.clone(),
        None => {
            eprintln!("[FATAL] server identifiers not initialized: call init_server_identifiers(<config.json>) before using transport");
            std::process::exit(1);
        }
    }
}

/// Parse client's requested services from config file (for service discovery).
/// Returns (service_id, method_ids) for the first service in the config.
/// This is used by clients to know which service to discover via SD.
pub fn client_requested_service(config_file: &str) -> Option<(u16, Vec<u16>)> {
    let config_path = transport_config_path(config_file);
    let config_str = std::fs::read_to_string(config_path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&config_str).ok()?;

    let service_id_str = v.get("services")?.get(0)?.get("service")?.as_str()?;
    let service_id = u16::from_str_radix(service_id_str.trim_start_matches("0x"), 16).ok()?;

    let methods_array = v.get("services")?.get(0)?.get("methods")?.as_array()?;
    let mut methods = Vec::new();
    for method_val in methods_array {
        if let Some(method_str) = method_val.as_str() {
            if let Ok(method_id) = u16::from_str_radix(method_str.trim_start_matches("0x"), 16) {
                methods.push(method_id);
            }
        }
    }

    Some((service_id, methods))
}

/// Must be called before initializing UPTransportVsomeip in client/server binaries.
///
/// Safety contract (Rust 2024): call this only during process startup,
/// before spawning worker threads / Tokio runtime.
///
/// Cosa fa:
/// 1. Costruisce una config runtime tramite `runtime_transport_config_path`.
/// 2. Imposta `VSOMEIP_CONFIGURATION` una sola volta per processo (`Once`).
/// 3. Attende brevemente per dare tempo al layer nativo di leggere la variabile.
pub fn setup_vsomeip_config(sd_file_name: &str) {
    SETUP_ONCE.call_once(|| {
        let runtime_path = runtime_transport_config_path(sd_file_name);
        // SAFETY: this function is intended to run once at startup, before
        // any background threads or runtime workers are created.
        unsafe {
            std::env::set_var(
                "VSOMEIP_CONFIGURATION",
                runtime_path.to_string_lossy().to_string(),
            );
        }
        // Small delay helps native init code read env vars reliably.
        thread::sleep(Duration::from_millis(50));
    });
}