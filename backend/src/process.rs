use crate::config::atomic;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stop_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}
pub fn install_signals() {
    unsafe {
        libc::signal(libc::SIGTERM, stop_signal as *const () as usize);
        libc::signal(libc::SIGINT, stop_signal as *const () as usize);
    }
}
pub fn stopping() -> bool {
    STOP.load(Ordering::SeqCst)
}
#[derive(Serialize, Deserialize)]
struct Record {
    pid: u32,
    started: String,
    executable: PathBuf,
}
fn start_time(pid: u32) -> Result<String> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    Ok(text
        .rsplit_once(')')
        .context("invalid proc stat")?
        .1
        .split_whitespace()
        .nth(19)
        .context("missing start time")?
        .into())
}
fn record(child: &Child, exe: &Path, data: &Path) -> Result<()> {
    atomic(
        &data.join("core.json"),
        &serde_json::to_vec(&Record {
            pid: child.id(),
            started: start_time(child.id())?,
            executable: exe.canonicalize()?,
        })?,
    )
}
pub fn cleanup(data: &Path) -> Result<()> {
    let path = data.join("core.json");
    if !path.exists() {
        return Ok(());
    }
    let r: Record = serde_json::from_slice(&fs::read(&path)?)?;
    let owned = || {
        start_time(r.pid).ok().as_ref() == Some(&r.started)
            && fs::read_link(format!("/proc/{}/exe", r.pid)).ok().as_ref() == Some(&r.executable)
    };
    if owned() {
        unsafe {
            libc::kill(r.pid as i32, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while owned() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
        ensure!(!owned(), "Mihomo 未完成网络清理，请重试停用");
    }
    let _ = fs::remove_file(path);
    Ok(())
}
/// Classify bounded log tails; never echo provider URLs or node credentials to RPC.
pub fn startup_hint(data: &Path) -> String {
    let Ok(mut file) = fs::File::open(data.join("mihomo.log")) else {
        return String::new();
    };
    let _ = file
        .seek(SeekFrom::End(-16384))
        .or_else(|_| file.seek(SeekFrom::Start(0)));
    let mut bytes = Vec::new();
    let _ = file.take(16384).read_to_end(&mut bytes);
    let log = String::from_utf8_lossy(&bytes).to_ascii_lowercase();
    for (pattern, reason) in [
        (
            "operation not permitted",
            "系统拒绝网络操作（operation not permitted）",
        ),
        (
            "permission denied",
            "文件或网络设备权限不足（permission denied）",
        ),
        (
            "address already in use",
            "监听地址已被占用（address already in use）",
        ),
        ("invalid argument", "网络设备或参数无效（invalid argument）"),
        (
            "no such file or directory",
            "所需文件或设备不存在（no such file or directory）",
        ),
        (
            "device or resource busy",
            "网络设备正被占用（device or resource busy）",
        ),
    ] {
        if log.contains(pattern) {
            return format!("；日志提示：{reason}");
        }
    }
    String::new()
}

pub struct Core {
    child: Child,
    pipe: Option<ChildStdin>,
    data: PathBuf,
}
impl Core {
    pub fn launch(exe: &Path, data: &Path) -> Result<Self> {
        let mut child = Command::new(std::env::current_exe()?)
            .arg("--supervise")
            .arg(exe)
            .arg(data)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?;
        let pipe = child.stdin.take();
        Ok(Self {
            child,
            pipe,
            data: data.to_owned(),
        })
    }
    pub fn alive(&mut self) -> bool {
        self.child.try_wait().ok().is_some_and(|s| s.is_none())
    }
    pub fn stop(&mut self) -> Result<()> {
        self.pipe.take();
        let deadline = Instant::now() + Duration::from_secs(6);
        while self.child.try_wait()?.is_none() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
        cleanup(&self.data)?;
        ensure!(self.child.try_wait()?.is_some(), "守护进程未退出");
        Ok(())
    }
}
impl Drop for Core {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// This process owns the core. An inherited pipe closes on any backend exit,
/// including SIGKILL. Detach from the host process group to allow TUN cleanup.
pub fn supervise(exe: &Path, data: &Path) -> Result<()> {
    unsafe {
        libc::setsid();
    }
    install_signals();
    let log = data.join("mihomo.log");
    if log.exists() {
        let _ = fs::rename(&log, data.join("mihomo.previous.log"));
    }
    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)?;
    let mut child = Command::new(exe)
        .arg("-d")
        .arg(data.join("run"))
        .arg("-f")
        .arg(data.join("runtime.json"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", data)
        .env("SAFE_PATHS", exe.parent().context("payload")?)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdout = child.stdout.take().context("stdout")?;
    let mut stderr = child.stderr.take().context("stderr")?;
    // Keep bounded diagnostic tails; never send node credentials to RPC stdout.
    let mut log1 = file.try_clone()?;
    thread::spawn(move || {
        let mut bytes = [0u8; 4096];
        while let Ok(n) = stdout.read(&mut bytes) {
            if n == 0 {
                break;
            }
            if log1.metadata().is_ok_and(|m| m.len() > 1024 * 1024) {
                let _ = log1.set_len(0);
            }
            let _ = log1.write_all(&bytes[..n]);
        }
    });
    thread::spawn(move || {
        let mut file = file;
        let mut bytes = [0u8; 4096];
        while let Ok(n) = stderr.read(&mut bytes) {
            if n == 0 {
                break;
            }
            if file.metadata().is_ok_and(|m| m.len() > 1024 * 1024) {
                let _ = file.set_len(0);
            }
            let _ = file.write_all(&bytes[..n]);
        }
    });
    if let Err(e) = record(&child, exe, data) {
        unsafe {
            libc::kill(child.id() as i32, libc::SIGTERM);
        }
        let _ = child.wait();
        return Err(e);
    }
    loop {
        if child.try_wait()?.is_some() {
            let _ = fs::remove_file(data.join("core.json"));
            return Ok(());
        }
        let mut fd = libc::pollfd {
            fd: 0,
            events: libc::POLLIN | libc::POLLHUP,
            revents: 0,
        };
        unsafe {
            libc::poll(&mut fd, 1, 100);
        }
        if stopping() || fd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
            break;
        }
        if fd.revents & libc::POLLIN != 0 {
            let mut b = [0u8; 1];
            if std::io::stdin().read(&mut b)? == 0 {
                break;
            }
        }
    }
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.try_wait()?.is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    // Retain journal on a hung core; standalone cleanup reports that failure.
    ensure!(child.try_wait()?.is_some(), "Mihomo 停止超时，保留清理记录");
    let _ = fs::remove_file(data.join("core.json"));
    Ok(())
}
pub fn validate(exe: &Path, data: &Path, config: &Path) -> Result<()> {
    let mut cmd = Command::new(exe);
    cmd.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", data)
        .env("SAFE_PATHS", exe.parent().context("payload")?);
    // A validation child cannot outlive its backend if the RPC times out.
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            Ok(())
        });
    }
    let mut child = cmd
        .arg("-t")
        .arg("-d")
        .arg(data.join("run"))
        .arg("-f")
        .arg(config)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("无法执行 Mihomo 配置校验（请检查执行权限或挂载限制）")?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(status.success(), "Mihomo 配置校验失败，请检查节点和规则");
            return Ok(());
        }
        if Instant::now() > deadline {
            child.kill()?;
            child.wait()?;
            anyhow::bail!("配置校验超时；请检查规则和集合配置");
        }
        thread::sleep(Duration::from_millis(30));
    }
}
