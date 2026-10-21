use std::collections::HashSet;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Lower-cased image names of all running processes. `tasklist /FO CSV` prints the
/// image name as the first quoted field regardless of the system language.
pub fn running_image_names() -> HashSet<String> {
    let output = Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    let mut names = HashSet::new();
    if let Ok(out) = output {
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            if let Some(name) = line.trim().strip_prefix('"').and_then(|l| l.split('"').next()) {
                names.insert(name.to_lowercase());
            }
        }
    }
    names
}

pub fn any_running(image_names: &[&str]) -> bool {
    let running = running_image_names();
    image_names
        .iter()
        .any(|name| running.contains(&name.to_lowercase()))
}

/// Force-closes the given images and waits until none of them is running.
pub fn stop_processes(image_names: &[&str], timeout_s: f64, label: &str) -> Result<bool, String> {
    if !any_running(image_names) {
        return Ok(false);
    }
    for name in image_names {
        let _ = Command::new("taskkill")
            .args(["/IM", name, "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
    if wait_until(timeout_s, || !any_running(image_names)) {
        Ok(true)
    } else {
        Err(format!("{} 未能在规定时间内退出", label))
    }
}

/// Polls `condition` every 250 ms until it holds or the timeout expires.
pub fn wait_until(timeout_s: f64, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs_f64(timeout_s);
    loop {
        if condition() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        sleep(Duration::from_millis(250));
    }
}
