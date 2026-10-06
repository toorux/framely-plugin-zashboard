use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

pub const TUN_DEVICE: &str = "zashboard-tun";
// Linux reserves one byte of IFNAMSIZ for the terminating NUL.
const _: () = assert!(TUN_DEVICE.len() < libc::IFNAMSIZ);

pub const MAX_PROFILE: usize = 4 * 1024 * 1024;
pub const LOCAL_NETS: &[&str] = &[
    "127.0.0.0/8",
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "169.254.0.0/16",
    "224.0.0.0/4",
    "255.255.255.255/32",
    "::1/128",
    "fc00::/7",
    "fe80::/10",
    "ff00::/8",
];

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub enabled: bool,
    pub mode: String,
    pub name: String,
    pub subscription: String,
    pub updated_at: u64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: "rule".into(),
            name: String::new(),
            subscription: String::new(),
            updated_at: 0,
        }
    }
}
pub fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("new");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(tmp, path)?;
    fs::File::open(path.parent().context("missing parent")?)?.sync_all()?;
    Ok(())
}
pub fn secret() -> Result<String> {
    let mut b = [0u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}
pub fn mode(value: &str) -> Result<()> {
    ensure!(
        ["rule", "global", "direct"].contains(&value),
        "代理模式无效"
    );
    Ok(())
}
pub fn subscription_url(value: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(value).context("订阅地址无效")?;
    ensure!(
        url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none(),
        "订阅必须是无账号密码的 HTTPS 地址"
    );
    Ok(url)
}

/// Copy supported routing fields only. Incoming listeners, TUN, controller,
/// scripts and local provider paths never control the root process.
pub fn normalize(text: &str) -> Result<Value> {
    ensure!(
        !text.is_empty() && text.len() <= MAX_PROFILE,
        "配置需为 1 B–4 MiB 的 Clash YAML"
    );
    let parsed: Value = serde_yaml::from_str(text).context("Clash YAML 无法解析")?;
    let map = parsed
        .as_object()
        .context("配置必须是 YAML 对象，不支持节点链接或 Base64 订阅")?;
    let mut out = json!({});
    for key in [
        "proxies",
        "proxy-groups",
        "proxy-providers",
        "rule-providers",
        "rules",
        "sub-rules",
    ] {
        if let Some(v) = map.get(key) {
            out[key] = v.clone();
        }
    }
    ensure!(
        out["proxies"].as_array().is_some_and(|a| !a.is_empty())
            || out["proxy-providers"]
                .as_object()
                .is_some_and(|a| !a.is_empty()),
        "配置中没有代理节点或代理集合"
    );
    for key in ["proxy-providers", "rule-providers"] {
        if let Some(providers) = out.get_mut(key) {
            let providers = providers.as_object_mut().context("集合必须为对象")?;
            for (i, (_name, provider)) in providers.iter_mut().enumerate() {
                let p = provider.as_object_mut().context("集合配置无效")?;
                match p.get("type").and_then(Value::as_str) {
                    Some("http") => {
                        subscription_url(
                            p.get("url")
                                .and_then(Value::as_str)
                                .context("集合缺少 URL")?,
                        )?;
                        p.insert("path".into(), json!(format!("providers/{key}-{i}.yaml")));
                    }
                    Some("inline") => {
                        p.remove("path");
                    }
                    _ => bail!("只支持 HTTPS 或 inline 集合；请将本地文件集合转换后导入"),
                }
            }
        }
    }
    Ok(out)
}
pub fn runtime(
    profile: &Value,
    settings: &Settings,
    port: u16,
    token: &str,
    ui: &Path,
    gateway: u16,
) -> Value {
    let mut c = profile.clone();
    c["mode"] = json!(settings.mode);
    c["mixed-port"] = json!(0);
    c["allow-lan"] = json!(false);
    c["bind-address"] = json!("127.0.0.1");
    c["external-controller"] = json!(format!("127.0.0.1:{port}"));
    c["secret"] = json!(token);
    c["external-ui"] = json!(ui);
    c["external-controller-cors"] = json!({"allow-origins":[format!("http://localhost:{port}"),format!("http://127.0.0.1:{port}"),format!("http://localhost:{gateway}")],"allow-private-network":true});
    c["log-level"] = json!("warning");
    c["ipv6"] = json!(true);
    c["geodata-mode"] = json!(true);
    c["geo-auto-update"] = json!(false);
    c["profile"] = json!({"store-selected":true,"store-fake-ip":false});
    c["tun"] = json!({"enable":settings.enabled,"device":TUN_DEVICE,"stack":"mixed","auto-route":true,"auto-redirect":false,"auto-detect-interface":true,"strict-route":false,"dns-hijack":["any:53","tcp://any:53"],"route-exclude-address":LOCAL_NETS});
    c["dns"] = json!({"enable":true,"listen":"127.0.0.1:0","ipv6":true,"enhanced-mode":"fake-ip","fake-ip-range":"198.18.0.1/16","fake-ip-filter":["+.lan","+.local","localhost","+.home.arpa"],"default-nameserver":["223.5.5.5","1.1.1.1"],"nameserver":["223.5.5.5","1.1.1.1"],"proxy-server-nameserver":["223.5.5.5","1.1.1.1"]});
    let mut rules = vec![
        json!("DOMAIN-SUFFIX,local,DIRECT"),
        json!("DOMAIN-SUFFIX,lan,DIRECT"),
    ];
    for net in LOCAL_NETS {
        rules.push(json!(format!(
            "{},{net},DIRECT,no-resolve",
            if net.contains(':') {
                "IP-CIDR6"
            } else {
                "IP-CIDR"
            }
        )));
    }
    if let Some(original) = profile["rules"].as_array() {
        rules.extend(original.iter().cloned());
    }
    if !rules
        .iter()
        .any(|v| v.as_str().is_some_and(|s| s.starts_with("MATCH,")))
    {
        rules.push(json!("MATCH,DIRECT"));
    }
    c["rules"] = json!(rules);
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locks_down_root_config() {
        let p = normalize("proxies: [{name: test, type: socks5, server: 127.0.0.1, port: 1080}]\nexternal-controller: 0.0.0.0:9090\nmixed-port: 8888\ntun: {enable: true}\nproxy-providers: {remote: {type: http, url: 'https://example.org/profile', path: /etc/shadow}}\nrules: ['MATCH,test']").unwrap();
        assert!(p.get("tun").is_none());
        assert_eq!(
            p["proxy-providers"]["remote"]["path"],
            "providers/proxy-providers-0.yaml"
        );
        let c = runtime(
            &p,
            &Settings::default(),
            2345,
            "secret",
            Path::new("/tmp/ui"),
            3456,
        );
        assert_eq!(c["tun"]["enable"], false);
        assert_eq!(c["tun"]["device"], TUN_DEVICE);
        assert!(TUN_DEVICE.len() < libc::IFNAMSIZ);
        assert_eq!(c["mixed-port"], 0);
        assert_eq!(c["external-controller"], "127.0.0.1:2345");
        assert!(
            c["rules"]
                .as_array()
                .unwrap()
                .iter()
                .position(|v| v == "IP-CIDR,192.168.0.0/16,DIRECT,no-resolve")
                .unwrap()
                < c["rules"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .position(|v| v == "MATCH,test")
                    .unwrap()
        );
    }
    #[test]
    fn rejects_file_providers_and_invalid_profiles() {
        assert!(normalize("proxies: []").is_err());
        assert!(normalize("- ss://node").is_err());
        assert!(normalize("proxy-providers: {local: {type: file, path: /etc/shadow}}").is_err());
        assert!(subscription_url("http://example.org").is_err());
        assert!(subscription_url("https://user:password@example.org").is_err());
    }
}
