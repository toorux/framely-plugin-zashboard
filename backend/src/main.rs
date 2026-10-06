mod config;
mod process;
mod speed;
mod web;
use anyhow::{ensure, Context, Result};
use config::{atomic, Settings};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use reqwest::{blocking::Client, Method};
use serde_json::{json, Value};
use std::{
    fs,
    io::{self, BufRead, Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Engine {
    data: PathBuf,
    payload: PathBuf,
    settings: Settings,
    profile: Option<Value>,
    core: Option<process::Core>,
    api_port: u16,
    token: String,
    client: Client,
    web: web::Web,
    initialized: bool,
    error: Option<String>,
    upload: Option<(String, String)>,
    speed: Option<speed::Job>,
}
impl Engine {
    fn new(data: PathBuf, payload: PathBuf) -> Result<Self> {
        fs::create_dir_all(&data)?;
        fs::set_permissions(&data, fs::Permissions::from_mode(0o700))?;
        process::cleanup(&data)?;
        let core = payload.join("mihomo");
        let metadata = fs::symlink_metadata(&core).context("无法读取 Mihomo 文件信息")?;
        ensure!(metadata.file_type().is_file(), "Mihomo 必须是普通文件");
        // Framely's installer marks only backend/lifecycle entries executable.
        if metadata.permissions().mode() & 0o111 != 0o111 {
            fs::set_permissions(&core, fs::Permissions::from_mode(0o755))
                .context("无法恢复 Mihomo 执行权限")?;
        }
        fs::create_dir_all(data.join("run/providers"))?;
        for name in ["geoip.dat", "geosite.dat", "Country.mmdb"] {
            if !data.join("run").join(name).exists() {
                fs::copy(payload.join("geo").join(name), data.join("run").join(name))
                    .with_context(|| format!("缺少离线规则数据库 {name}"))?;
            }
        }
        let stored: Value = if data.join("state.json").exists() {
            serde_json::from_slice(&fs::read(data.join("state.json"))?)?
        } else {
            json!({"settings": Settings::default(), "profile": null})
        };
        let settings: Settings = serde_json::from_value(stored["settings"].clone())?;
        config::mode(&settings.mode)?;
        let profile = if stored["profile"].is_null() {
            None
        } else {
            Some(stored["profile"].clone())
        };
        let web = web::Web::new(&payload)?;
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()?;
        Ok(Self {
            data,
            payload,
            settings,
            profile,
            core: None,
            api_port: 0,
            token: String::new(),
            client,
            web,
            initialized: false,
            error: None,
            upload: None,
            speed: None,
        })
    }
    fn save(&self) -> Result<()> {
        atomic(
            &self.data.join("state.json"),
            &serde_json::to_vec(&json!({"settings":self.settings,"profile":self.profile}))?,
        )
        .context("无法保存配置状态")
    }
    fn api(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        ensure!(self.core.is_some(), "代理内核尚未运行");
        let mut request = self
            .client
            .request(method, format!("http://127.0.0.1:{}{path}", self.api_port))
            .bearer_auth(&self.token);
        if let Some(b) = body {
            request = request.json(&b);
        }
        let response = request.send().context("无法连接代理内核")?;
        ensure!(
            response.status().is_success(),
            "代理内核拒绝操作（{}）",
            response.status().as_u16()
        );
        let bytes = response.bytes()?;
        if bytes.is_empty() {
            Ok(json!({}))
        } else {
            Ok(serde_json::from_slice(&bytes)?)
        }
    }
    fn stop(&mut self) -> Result<()> {
        self.speed = None;
        *self.web.dashboard.lock().unwrap() = None;
        if let Some(mut c) = self.core.take() {
            c.stop()?;
        }
        process::cleanup(&self.data)
    }
    fn start(&mut self) -> Result<()> {
        self.stop()?;
        let Some(profile) = &self.profile else {
            return Ok(());
        };
        self.token = config::secret()?;
        if self.api_port == 0 {
            self.api_port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
        }
        if self.settings.enabled {
            ensure!(unsafe { libc::geteuid() } == 0, "TUN 需要 root 后端");
            ensure!(
                std::path::Path::new("/dev/net/tun").exists(),
                "设备缺少 /dev/net/tun"
            );
            // Block both current and legacy interfaces during the ID transition.
            for device in [config::TUN_DEVICE, "framely-clash", "framely-zashboa"] {
                ensure!(
                    !std::path::Path::new("/sys/class/net").join(device).exists(),
                    "已有 {device} 网卡，请先停用旧代理或完成残留清理"
                );
            }
        }
        let runtime = config::runtime(
            profile,
            &self.settings,
            self.api_port,
            &self.token,
            &self.payload.join("dashboard"),
            self.web.port,
        );
        atomic(
            &self.data.join("runtime.json"),
            &serde_json::to_vec(&runtime)?,
        )?;
        self.core = Some(process::Core::launch(
            &self.payload.join("mihomo"),
            &self.data,
        )?);
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut reason = String::from("控制 API 未响应");
        while Instant::now() < deadline {
            if !self.core.as_mut().is_some_and(|c| c.alive()) {
                reason = "内核进程在启动期间退出".into();
                break;
            }
            if self.api(Method::GET, "/version", None).is_ok() {
                // The controller binds before asynchronous config application.
                // Retry transient errors instead of leaving a partially started core.
                let snapshot = self.api(Method::GET, "/configs", None).and_then(|general| {
                    self.api(Method::GET, "/proxies", None)
                        .map(|proxies| (general, proxies))
                });
                let Ok((general, proxies)) = snapshot else {
                    reason = "控制 API 已启动，但配置状态暂不可读取".into();
                    thread::sleep(Duration::from_millis(100));
                    continue;
                };
                let configured = proxies["proxies"]["GLOBAL"].is_object()
                    && general["mode"] == self.settings.mode
                    && general["tun"]["enable"].as_bool() == Some(self.settings.enabled);
                let tun_ready = !self.settings.enabled
                    || std::path::Path::new("/sys/class/net")
                        .join(config::TUN_DEVICE)
                        .exists();
                reason = if !configured {
                    "控制 API 已启动，但节点、模式或 TUN 配置尚未生效".into()
                } else if !tun_ready {
                    format!("TUN 已启用，但网卡 {} 未创建", config::TUN_DEVICE)
                } else {
                    String::new()
                };
                if configured && tun_ready {
                    *self.web.dashboard.lock().unwrap() = Some(web::Dashboard {
                        port: self.api_port,
                        token: self.token.clone(),
                    });
                    self.error = None;
                    return Ok(());
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
        self.stop()
            .with_context(|| format!("Mihomo 未就绪：{reason}；停止内核失败"))?;
        let hint = process::startup_hint(&self.data);
        anyhow::bail!("Mihomo 未就绪：{reason}{hint}。详情见插件数据目录中的 mihomo.log")
    }
    fn restart(&mut self) -> Result<()> {
        if let Err(e) = self.start() {
            self.settings.enabled = false;
            self.save()?;
            let _ = self.stop();
            self.error = Some(e.to_string());
            return Err(e);
        }
        Ok(())
    }
    fn status(&mut self) -> Result<Value> {
        if let Some(job) = &mut self.speed {
            job.poll();
        }
        if self.core.as_mut().is_some_and(|c| !c.alive()) {
            self.stop()?;
            self.settings.enabled = false;
            self.save()?;
            self.error = Some("代理内核已退出，代理已关闭；可重试启动".into());
        }
        let mut groups = vec![];
        let mut groups_bytes = 0;
        let mut groups_truncated = false;
        let mut connections = 0;
        let mut up = 0;
        let mut down = 0;
        if self.core.is_some() {
            if let Ok(proxies) = self.api(Method::GET, "/proxies", None) {
                if let Some(all) = proxies["proxies"].as_object() {
                    let order = self
                        .profile
                        .as_ref()
                        .and_then(|p| p["proxy-groups"].as_array());
                    let mut ordered: Vec<_> = all.iter().collect();
                    ordered.sort_by_key(|(name, _)| {
                        order
                            .and_then(|groups| {
                                groups
                                    .iter()
                                    .position(|g| g["name"].as_str() == Some(name.as_str()))
                            })
                            .unwrap_or(usize::MAX)
                    });
                    for (name, p) in ordered {
                        if p["type"] == "Selector" {
                            let mut route = vec![];
                            let mut current = p["now"].as_str();
                            let mut exit = None;
                            while let Some(node) = current {
                                if route.len() >= 16 || route.contains(&node) {
                                    break;
                                }
                                route.push(node);
                                let Some(proxy) = all.get(node) else {
                                    break;
                                };
                                current = proxy["now"].as_str();
                                if current.is_none() {
                                    exit = Some(node);
                                }
                            }
                            let delays = self
                                .speed
                                .as_ref()
                                .filter(|job| job.group == *name)
                                .map(|job| &job.delays);
                            let group = json!({"name":name,"now":p["now"],"all":p["all"],"route":route,"exit":exit,"delays":delays});
                            let size = serde_json::to_vec(&group)?.len();
                            if groups_bytes + size < 48000 {
                                groups_bytes += size;
                                groups.push(group);
                            } else {
                                groups_truncated = true;
                            }
                        }
                    }
                }
            }
            if let Ok(c) = self.api(Method::GET, "/connections", None) {
                connections = c["connections"].as_array().map_or(0, Vec::len);
                up = c["uploadTotal"].as_u64().unwrap_or(0);
                down = c["downloadTotal"].as_u64().unwrap_or(0);
            }
            if let Ok(c) = self.api(Method::GET, "/configs", None) {
                if let Some(m) = c["mode"].as_str() {
                    if m != self.settings.mode && config::mode(m).is_ok() {
                        self.settings.mode = m.into();
                        self.save()?;
                    }
                }
            }
        }
        let primary_group = groups
            .iter()
            .find(|g| g["name"] != "GLOBAL")
            .and_then(|g| g["name"].as_str());
        let speed_test = self.speed.as_ref().map(speed::Job::status);
        Ok(
            json!({"speedTest":speed_test,"primaryGroup":primary_group,"ready":self.initialized,"configured":self.profile.is_some(),"enabled":self.settings.enabled&&self.core.is_some(),"running":self.core.is_some(),"mode":self.settings.mode,"name":self.settings.name,"hasSubscription":!self.settings.subscription.is_empty(),"updatedAt":self.settings.updated_at,"groups":groups,"groupsTruncated":groups_truncated,"connections":connections,"uploadTotal":up,"downloadTotal":down,"error":self.error,"mihomo":"1.19.32","zashboard":"3.29.1"}),
        )
    }
    fn import(&mut self, text: &str, name: &str, subscription: &str) -> Result<()> {
        ensure!(
            !name.trim().is_empty() && name.len() <= 120,
            "配置名称需为 1–120 字节"
        );
        let profile = config::normalize(text)?;
        let runtime = config::runtime(
            &profile,
            &Settings::default(),
            1,
            "test",
            &self.payload.join("dashboard"),
            self.web.port,
        );
        let candidate = self.data.join("candidate.json");
        atomic(&candidate, &serde_json::to_vec(&runtime)?).context("无法写入待校验配置")?;
        let result = process::validate(&self.payload.join("mihomo"), &self.data, &candidate);
        let _ = fs::remove_file(candidate);
        result?;
        let previous = self.profile.clone();
        let old_settings = self.settings.clone();
        self.settings.name = name.trim().into();
        self.settings.subscription = subscription.into();
        self.settings.updated_at = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        self.profile = Some(profile);
        if let Err(e) = self.restart() {
            self.profile = previous;
            self.settings = old_settings;
            self.settings.enabled = false;
            let _ = self.start();
            let _ = self.save();
            self.error = Some(e.to_string());
            return Err(e);
        }
        self.save()?;
        Ok(())
    }
    fn download(url: &str) -> Result<String> {
        let url = config::subscription_url(url)?;
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(7))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 3
                    || config::subscription_url(attempt.url().as_str()).is_err()
                {
                    attempt.error("invalid subscription redirect")
                } else {
                    attempt.follow()
                }
            }))
            .build()?;
        let response = client
            .get(url)
            .header("User-Agent", "clash.meta/framely")
            .send()
            .map_err(|_| anyhow::anyhow!("订阅下载失败，请检查网络或地址"))?;
        ensure!(
            response.status().is_success(),
            "订阅服务器返回 HTTP {}",
            response.status().as_u16()
        );
        let mut bytes = vec![];
        response
            .take((config::MAX_PROFILE + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| anyhow::anyhow!("订阅读取超时或失败"))?;
        ensure!(bytes.len() <= config::MAX_PROFILE, "订阅超过 4 MiB");
        String::from_utf8(bytes).context("订阅不是 UTF-8 Clash YAML")
    }
    fn dispatch(&mut self, method: &str, p: &Value) -> Result<Value> {
        match method {
            "framely.lifecycle.start" => {
                self.initialized = true;
                self.restart()?;
                return Ok(json!({"ready":true}));
            }
            "framely.lifecycle.stop" => {
                self.initialized = false;
                self.upload = None;
                self.stop()?;
                return Ok(json!({"cleaned":true}));
            }
            "status.get" => return self.status(),
            _ => ensure!(self.initialized, "后端尚未完成启动生命周期"),
        }
        let string = |key: &str| p[key].as_str().with_context(|| format!("缺少字段 {key}"));
        match method {
            "service.set" => {
                let enabled = p["enabled"].as_bool().context("enabled 必须为布尔值")?;
                ensure!(!enabled || self.profile.is_some(), "请先导入配置");
                self.settings.enabled = enabled;
                self.save()?;
                self.restart()?;
            }
            "service.retry" => {
                self.restart()?;
            }
            "mode.set" => {
                let m = string("mode")?;
                config::mode(m)?;
                if self.core.is_some() {
                    self.api(Method::PATCH, "/configs", Some(json!({"mode":m})))?;
                }
                self.settings.mode = m.into();
                self.save()?;
            }
            "proxy.testAll" => {
                if let Some(job) = &mut self.speed {
                    job.poll();
                }
                ensure!(
                    !self.speed.as_ref().is_some_and(speed::Job::running),
                    "节点测速正在进行，请等待完成"
                );
                let group = string("group")?;
                let proxies = self.api(Method::GET, "/proxies", None)?;
                ensure!(
                    proxies["proxies"][group]["type"] == "Selector",
                    "策略组不可手动选择"
                );
                let choices = proxies["proxies"][group]["all"]
                    .as_array()
                    .context("策略组没有节点")?;
                let mut names = Vec::new();
                for name in choices.iter().filter_map(Value::as_str) {
                    if !names.iter().any(|n| n == name) {
                        names.push(name.to_owned());
                    }
                }
                ensure!(!names.is_empty(), "策略组没有节点");
                self.speed = Some(speed::Job::start(
                    self.client.clone(),
                    format!("http://127.0.0.1:{}", self.api_port),
                    self.token.clone(),
                    group.to_owned(),
                    names,
                ));
            }
            "proxy.select" => {
                let group = string("group")?;
                let name = string("name")?;
                let path = format!("/proxies/{}", utf8_percent_encode(group, NON_ALPHANUMERIC));
                self.api(Method::PUT, &path, Some(json!({"name":name})))?;
            }
            "profile.import" => {
                self.import(string("yaml")?, string("name")?, "")?;
            }
            "profile.begin" => {
                let name = string("name")?;
                ensure!(!name.trim().is_empty() && name.len() <= 120, "配置名称无效");
                self.upload = Some((name.into(), String::new()));
                return Ok(json!({"received":0}));
            }
            "profile.chunk" => {
                let text = string("text")?;
                ensure!(text.len() <= 32768, "分片过大");
                let (_, buffer) = self.upload.as_mut().context("请先开始导入")?;
                ensure!(
                    buffer.len() + text.len() <= config::MAX_PROFILE,
                    "配置超过 4 MiB"
                );
                buffer.push_str(text);
                return Ok(json!({"received":buffer.len()}));
            }
            "profile.commit" => {
                let (name, text) = self.upload.take().context("没有待导入配置")?;
                self.import(&text, &name, "")?;
            }
            "profile.cancel" => {
                self.upload = None;
                return Ok(json!({"cancelled":true}));
            }
            "subscription.set" => {
                let url = string("url")?;
                let text = Self::download(url)?;
                self.import(&text, string("name")?, url)?;
            }
            "subscription.update" => {
                ensure!(!self.settings.subscription.is_empty(), "未保存订阅地址");
                let url = self.settings.subscription.clone();
                let name = self.settings.name.clone();
                let text = Self::download(&url)?;
                self.import(&text, &name, &url)?;
            }
            "browser.get" => {
                return Ok(
                    json!({"url":format!("http://localhost:{}/settings#{}",self.web.port,self.web.token)}),
                );
            }
            "window.get" => {
                let key = string("window")?;
                ensure!(matches!(key, "main" | "dashboard"), "窗口不存在");
                return Ok(
                    json!({"url":format!("http://localhost:{}/framely-window/{key}",self.web.port)}),
                );
            }
            _ => anyhow::bail!("未知方法：{method}"),
        }
        self.status()
    }
}
fn output(v: Value) {
    let mut stdout = io::stdout().lock();
    let _ = serde_json::to_writer(&mut stdout, &v);
    let _ = writeln!(stdout);
    let _ = stdout.flush();
}
fn run() -> Result<()> {
    unsafe {
        libc::umask(0o077);
    }
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "--supervise") {
        return process::supervise(
            &PathBuf::from(args.get(2).context("core")?),
            &PathBuf::from(args.get(3).context("data")?),
        );
    }
    let data =
        PathBuf::from(std::env::var("FRAMELY_DATA_DIR").context("FRAMELY_DATA_DIR required")?);
    if args.get(1).is_some_and(|s| s == "--cleanup") || std::env::var("FRAMELY_LIFECYCLE").is_ok() {
        return process::cleanup(&data);
    }
    process::install_signals();
    fs::create_dir_all(&data)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(data.join("backend.lock"))?;
    use std::os::fd::AsRawFd;
    ensure!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "已有后端正在使用该数据目录"
    );
    let payload = std::env::current_exe()?
        .parent()
        .context("payload")?
        .canonicalize()?;
    let mut engine = Engine::new(data, payload)?;
    let (tx, rx) = mpsc::sync_channel(16);
    thread::spawn(move || {
        let stdin = io::stdin();
        let mut reader = stdin.lock();
        loop {
            let mut bytes = vec![];
            match reader.by_ref().take(65537).read_until(b'\n', &mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if bytes.len() > 65536 {
                        let _ = tx.send(Err("请求超过 64 KiB".to_string()));
                        break;
                    }
                    if tx
                        .send(serde_json::from_slice::<Value>(&bytes).map_err(|e| e.to_string()))
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });
    loop {
        if process::stopping() {
            break;
        }
        if let Ok(command) = engine.web.requests.try_recv() {
            let result = engine.dispatch(
                command.request["method"].as_str().unwrap_or(""),
                &command.request["params"],
            );
            let _ = command.reply.send(match result {
                Ok(v) => json!({"result":v}),
                Err(e) => json!({"error":format!("{e:#}")}),
            });
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Ok(request)) => {
                let result =
                    engine.dispatch(request["method"].as_str().unwrap_or(""), &request["params"]);
                output(match result {
                    Ok(v) => json!({"id":request["id"],"result":v}),
                    Err(e) => json!({"id":request["id"],"error":format!("{e:#}")}),
                });
            }
            Ok(Err(error)) => output(json!({"id":null,"error":error})),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if engine.core.as_mut().is_some_and(|c| !c.alive()) {
                    let _ = engine.status();
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    engine.stop()?;
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("zashboard: {e:#}");
        std::process::exit(1);
    }
}
