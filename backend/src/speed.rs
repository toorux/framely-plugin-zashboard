use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use reqwest::blocking::Client;
use serde_json::{json, Map, Value};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc, Arc,
};

/// A bounded background batch keeps normal RPC and lifecycle actions responsive.
pub struct Job {
    pub group: String,
    total: usize,
    pub delays: Map<String, Value>,
    receiver: mpsc::Receiver<(String, Option<u64>)>,
    cancel: Arc<AtomicBool>,
}
impl Job {
    pub fn start(
        client: Client,
        base: String,
        token: String,
        group: String,
        names: Vec<String>,
    ) -> Self {
        let total = names.len();
        let names = Arc::new(names);
        let cursor = Arc::new(AtomicUsize::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();
        for _ in 0..total.min(4) {
            let (client, base, token) = (client.clone(), base.clone(), token.clone());
            let (names, cursor, cancel, sender) = (
                names.clone(),
                cursor.clone(),
                cancel.clone(),
                sender.clone(),
            );
            std::thread::spawn(move || {
                while !cancel.load(Ordering::Relaxed) {
                    let index = cursor.fetch_add(1, Ordering::Relaxed);
                    let Some(name) = names.get(index) else { break };
                    let url = format!("{base}/proxies/{}/delay?timeout=1500&url=https%3A%2F%2Fwww.gstatic.com%2Fgenerate_204&expected=204", utf8_percent_encode(name, NON_ALPHANUMERIC));
                    let delay = client
                        .get(url)
                        .bearer_auth(&token)
                        .send()
                        .ok()
                        .filter(|r| r.status().is_success())
                        .and_then(|r| r.json::<Value>().ok())
                        .and_then(|v| v["delay"].as_u64())
                        .filter(|n| *n > 0);
                    if sender.send((name.clone(), delay)).is_err() {
                        break;
                    }
                }
            });
        }
        Self {
            group,
            total,
            delays: Map::new(),
            receiver,
            cancel,
        }
    }
    pub fn poll(&mut self) {
        while let Ok((name, delay)) = self.receiver.try_recv() {
            self.delays.insert(name, json!(delay));
        }
    }
    pub fn running(&self) -> bool {
        self.delays.len() < self.total
    }
    pub fn status(&self) -> Value {
        json!({"group":self.group,"total":self.total,"completed":self.delays.len(),"running":self.running()})
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn batch_collects_each_node_and_keeps_failures() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let base = format!("http://{}", server.server_addr());
        let worker = std::thread::spawn(move || {
            for _ in 0..3 {
                let req = server
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap()
                    .unwrap();
                assert!(req.url().contains("timeout=1500"));
                assert!(req
                    .headers()
                    .iter()
                    .any(|h| h.field.equiv("Authorization") && h.value.as_str() == "Bearer test"));
                let response = if req.url().starts_with("/proxies/slow/") {
                    tiny_http::Response::from_string("{\"delay\":80}")
                } else if req.url().starts_with("/proxies/fast/") {
                    tiny_http::Response::from_string("{\"delay\":10}")
                } else {
                    tiny_http::Response::from_string("{\"message\":\"timeout\"}")
                        .with_status_code(504)
                };
                req.respond(response).unwrap();
            }
        });
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let mut job = Job::start(
            client,
            base,
            "test".into(),
            "group".into(),
            vec!["slow".into(), "fast".into(), "failed".into()],
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        while job.running() && Instant::now() < deadline {
            job.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!job.running());
        assert_eq!(job.delays["fast"], 10);
        assert_eq!(job.delays["slow"], 80);
        assert!(job.delays["failed"].is_null());
        assert_eq!(job.status()["completed"], 3);
        worker.join().unwrap();
    }
}
