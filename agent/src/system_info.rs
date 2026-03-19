use serde_json::json;
use sysinfo::System;

fn format_mac(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(":")
}

pub fn collect_agent_info() -> serde_json::Value {
    let mut sys = System::new_all();
    sys.refresh_all();

    let hostname = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "unknown".into());
    let os_name = System::name().unwrap_or_else(|| "Windows".into());
    let os_version = System::os_version();
    let os_long_version = System::long_os_version();
    let kernel_version = System::kernel_version();

    let cpu_brand = sys
        .cpus()
        .first()
        .map(|c| c.brand().to_string())
        .unwrap_or_default();
    let cpu_cores = sys.cpus().len() as u32;

    // sysinfo returns memory in KiB.
    let total_mem_mb = (sys.total_memory() / 1024) as u64;
    let used_mem_mb = (sys.used_memory() / 1024) as u64;

    let adapters = ipconfig::get_adapters()
        .ok()
        .map(|list| {
            list.into_iter()
                .map(|a| {
                    let ips: Vec<String> = a
                        .ip_addresses()
                        .iter()
                        .map(|ip| ip.to_string())
                        .collect();
                    let gateways: Vec<String> = a
                        .gateways()
                        .iter()
                        .map(|ip| ip.to_string())
                        .collect();
                    let dns: Vec<String> = a.dns_servers().iter().map(|ip| ip.to_string()).collect();
                    let mac = a
                        .physical_address()
                        .map(format_mac)
                        .unwrap_or_default();

                    json!({
                        "name": a.friendly_name(),
                        "description": a.description(),
                        "mac": mac,
                        "ips": ips,
                        "gateways": gateways,
                        "dns": dns,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    json!({
        "type": "agent_info",
        "hostname": hostname,
        "os_name": os_name,
        "os_version": os_version,
        "os_long_version": os_long_version,
        "kernel_version": kernel_version,
        "cpu_brand": cpu_brand,
        "cpu_cores": cpu_cores,
        "memory_total_mb": total_mem_mb,
        "memory_used_mb": used_mem_mb,
        "adapters": adapters,
        "ts": crate::unix_timestamp_secs(),
    })
}

