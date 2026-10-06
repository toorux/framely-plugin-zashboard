use anyhow::{Context, Result};
use std::{
    io::Read,
    net::TcpListener,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::Duration,
};
use tiny_http::{Header, Method, Response, Server, StatusCode};

#[derive(Clone)]
pub struct Dashboard {
    pub port: u16,
    pub token: String,
}
pub struct BrowserRequest {
    pub request: serde_json::Value,
    pub reply: mpsc::SyncSender<serde_json::Value>,
}
pub struct Web {
    pub token: String,
    pub requests: mpsc::Receiver<BrowserRequest>,
    pub port: u16,
    pub dashboard: Arc<Mutex<Option<Dashboard>>>,
    done: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Web {
    pub fn new(payload: &Path) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let server =
            Server::from_listener(listener, None).map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let token = crate::config::secret()?;
        let auth = token.clone();
        let (sender, requests) = mpsc::sync_channel::<BrowserRequest>(16);
        let source = std::fs::read(payload.join("source.zip")).context("缺少源码包")?;
        let page = std::fs::read(payload.join("page.js")).context("缺少配置页面")?;
        let controls = std::fs::read(payload.join("window.js")).context("缺少 SDK 窗口脚本")?;
        let bootstrap = std::fs::read(payload.join("bridge.js")).context("缺少网页桥接脚本")?;
        let dashboard: Arc<Mutex<Option<Dashboard>>> = Arc::new(Mutex::new(None));
        let state = dashboard.clone();
        let done = Arc::new(AtomicBool::new(false));
        let stop = done.clone();
        let worker = thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let Ok(Some(mut req)) = server.recv_timeout(Duration::from_millis(200)) else {
                    continue;
                };
                let host = req
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("Host"))
                    .map(|h| h.value.as_str())
                    .unwrap_or("");
                let allowed =
                    host == format!("localhost:{port}") || host == format!("127.0.0.1:{port}");
                let origin = req
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("Origin"))
                    .map(|h| h.value.as_str());
                let own_origin = origin.is_none_or(|v| v == format!("http://{host}"));
                let (status, mime, body) = if !allowed || !own_origin {
                    (403, "text/plain", b"Local window only".to_vec())
                } else if req.url() == "/rpc" && req.method() == &Method::Post {
                    let authorized = req.headers().iter().any(|h| {
                        h.field.equiv("Authorization")
                            && h.value.as_str() == format!("Bearer {auth}")
                    });
                    if !authorized {
                        (
                            401,
                            "application/json",
                            r#"{"error":"浏览器会话无效，请重新打开配置入口"}"#.as_bytes().to_vec(),
                        )
                    } else {
                        let mut data = Vec::new();
                        let read = req.as_reader().take(65537).read_to_end(&mut data);
                        if read.is_err() || data.len() > 65536 {
                            (
                                413,
                                "application/json",
                                r#"{"error":"请求超过 64 KiB"}"#.as_bytes().to_vec(),
                            )
                        } else if let Ok(request) =
                            serde_json::from_slice::<serde_json::Value>(&data)
                        {
                            let method = request["method"].as_str().unwrap_or("");
                            if !matches!(
                                method,
                                "status.get"
                                    | "service.set"
                                    | "service.retry"
                                    | "mode.set"
                                    | "proxy.select"
                                    | "proxy.testAll"
                                    | "profile.begin"
                                    | "profile.chunk"
                                    | "profile.commit"
                                    | "profile.cancel"
                                    | "subscription.set"
                                    | "subscription.update"
                                    | "window.get"
                                    | "browser.get"
                            ) {
                                (
                                    403,
                                    "application/json",
                                    r#"{"error":"浏览器不支持此操作"}"#.as_bytes().to_vec(),
                                )
                            } else {
                                let (reply, receive) = mpsc::sync_channel(1);
                                if sender.try_send(BrowserRequest { request, reply }).is_err() {
                                    (
                                        503,
                                        "application/json",
                                        r#"{"error":"服务繁忙，请重试"}"#.as_bytes().to_vec(),
                                    )
                                } else {
                                    match receive.recv_timeout(Duration::from_secs(15)) {
                                        Ok(result) => (
                                            200,
                                            "application/json",
                                            serde_json::to_vec(&result).unwrap(),
                                        ),
                                        Err(_) => (
                                            503,
                                            "application/json",
                                            r#"{"error":"服务未响应，请重试"}"#.as_bytes().to_vec(),
                                        ),
                                    }
                                }
                            }
                        } else {
                            (
                                400,
                                "application/json",
                                r#"{"error":"无效请求"}"#.as_bytes().to_vec(),
                            )
                        }
                    }
                } else if req.method() != &Method::Get {
                    (403, "text/plain", b"Local window only".to_vec())
                } else if req.url() == "/settings" {
                    (200, "text/html", MANAGEMENT.as_bytes().to_vec())
                } else if req.url() == "/source.zip" {
                    (200, "application/zip", source.clone())
                } else if req.url() == "/page.js" {
                    (200, "text/javascript", page.clone())
                } else if req.url() == "/browser.js" {
                    (200, "text/javascript", BROWSER.as_bytes().to_vec())
                } else if req.url() == "/bridge.js" {
                    (200, "text/javascript", bootstrap.clone())
                } else if req.url() == "/window.js" {
                    (200, "text/javascript", controls.clone())
                } else if req.url() == "/relay.js" {
                    (200, "text/javascript", RELAY.as_bytes().to_vec())
                } else if matches!(
                    req.url(),
                    "/framely-window/main" | "/framely-window/dashboard"
                ) {
                    (
                        200,
                        "text/html",
                        wrapper(
                            state.lock().unwrap().as_ref(),
                            if req.url().ends_with("/dashboard") {
                                "dashboard"
                            } else {
                                "main"
                            },
                        )
                        .into_bytes(),
                    )
                } else {
                    (404, "text/plain", b"Not found".to_vec())
                };
                let mut response = Response::from_data(body).with_status_code(StatusCode(status));
                for (k,v) in [("Content-Type",mime),("Cache-Control","no-store"),("Referrer-Policy","no-referrer"),("X-Content-Type-Options","nosniff"),("Content-Security-Policy","default-src 'none'; script-src 'self'; connect-src 'self'; style-src 'unsafe-inline'; frame-src http://localhost:*; base-uri 'none'; form-action 'none'")] {
                    response.add_header(Header::from_bytes(k,v).unwrap());
                }
                let _ = req.respond(response);
            }
        });
        Ok(Self {
            token,
            requests,
            port,
            dashboard,
            done,
            worker: Some(worker),
        })
    }
}
impl Drop for Web {
    fn drop(&mut self) {
        self.done.store(true, Ordering::SeqCst);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}
fn wrapper(d: Option<&Dashboard>, key: &str) -> String {
    let content = if let Some(d) = d {
        format!(
            r#"<iframe id="dashboard" title="zashboard" src="http://localhost:{}/ui/#/setup?hostname=localhost&amp;port={}&amp;secret={}&amp;disableUpgradeCore=1&amp;disableTunMode=1" sandbox="allow-scripts allow-same-origin allow-forms allow-downloads" allow="clipboard-read; clipboard-write"></iframe>"#,
            d.port, d.port, d.token
        )
    } else {
        r#"<div class="empty"><h1>内核未运行</h1><p>请打开「配置管理」添加配置或重试内核，再刷新完整面板。</p></div>"#.to_owned()
    };
    format!(
        r#"<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="referrer" content="no-referrer"><title>zashboard</title><style>*{{box-sizing:border-box}}body{{margin:0;background:#14191e;color:#e5e9ed;font:16px system-ui;overflow:hidden}}header{{height:58px;padding:5px 18px;display:flex;align-items:center;gap:10px;border-bottom:1px solid #343c43}}header strong{{margin-right:auto}}button{{font:inherit;background:#28333c;color:inherit;border:1px solid #4c5c68;border-radius:8px;padding:8px 14px;min-height:48px;cursor:pointer}}iframe{{display:block;width:100%;height:calc(100dvh - 58px);border:0}}.empty{{padding:32px;color:#a6b3bd}}.empty h1{{font-size:22px;color:#e5e9ed}}#window-error{{position:fixed;top:58px;left:0;right:0;margin:0;padding:12px;background:#482e30;color:#ffd4d4}}</style><script src="/bridge.js" defer></script><script src="/window.js" defer></script><script src="/relay.js" defer></script></head><body data-window="{key}"><header><strong>zashboard</strong><button id="configure">配置管理</button><button id="refresh">刷新面板</button><button id="close" aria-label="关闭窗口">关闭</button></header><p id="window-error" role="alert" hidden></p>{content}</body></html>"#
    )
}
const RELAY: &str = r#"(()=>{
const frame=document.getElementById('dashboard');
window.addEventListener('message',async e=>{
if(!frame||e.source!==frame.contentWindow||e.origin!==new URL(frame.src).origin||e.data?.channel!=='framely.plugin'||!['keyboard','haptic'].includes(e.data.op)||!Number.isSafeInteger(e.data.id))return;
try{const result=await window.__framelyBridge.request(e.data.op,e.data.params);frame.contentWindow.postMessage({channel:'framely.reply',id:e.data.id,result},e.origin);}catch(error){frame.contentWindow.postMessage({channel:'framely.reply',id:e.data.id,error:String(error)},e.origin);}
});
})();"#;

const MANAGEMENT: &str = r#"<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>zashboard · 配置管理</title></head><body><div id="root"></div><script src="/browser.js"></script><script src="/page.js"></script></body></html>"#;
const BROWSER: &str = r#"(()=>{
const token=location.hash.slice(1);
if(token)sessionStorage.setItem('zashboard-session',token);
history.replaceState(null,'',location.pathname);
async function call(method,params={}){
const response=await fetch('/rpc',{method:'POST',headers:{'Content-Type':'application/json','Authorization':'Bearer '+sessionStorage.getItem('zashboard-session')},body:JSON.stringify({method,params})});
const data=await response.json();if(data.error)throw new Error(data.error);if(!response.ok)throw new Error('服务请求失败');return data.result;
}
window.__framelyBrowser=true;
window.__framelyBridge={subscribe:()=>()=>{},request:async(op,p={})=>{
if(op==='call')return call(p.method,p.params);
if(op==='window.open'){const popup=window.open('about:blank','_blank');try{const data=await call(p.window==='settings'?'browser.get':'window.get',{window:p.window});if(popup)popup.location.href=data.url;else throw new Error('请允许此网站打开弹出窗口');}catch(e){popup?.close();throw e;}return {};}
if(op==='window.close'||op==='ui.close'){window.close();return {};}
throw new Error('此操作需要 Framely');
}};
})();"#;
