use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use anyhow::{Result, Context, bail};

pub fn pid_path() -> PathBuf {
    let config_dir = Path::new(&crate::types::default_config_path())
        .parent()
        .unwrap()
        .to_path_buf();
    if !config_dir.exists() {
        let _ = fs::create_dir_all(&config_dir);
    }
    config_dir.join("ug.pid")
}

pub fn log_path() -> PathBuf {
    Path::new(&crate::types::default_config_path())
        .parent()
        .unwrap()
        .join("ug.log")
}

pub fn is_running() -> Option<u32> {
    let path = pid_path();
    if !path.exists() {
        return None;
    }
    let pid_str = fs::read_to_string(&path).ok()?;
    let pid: u32 = pid_str.trim().parse().ok()?;

    // Check if process exists (Unix-specific)
    #[cfg(unix)]
    {
        use std::process::Command;
        let status = Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok()?;
        if status.success() {
            Some(pid)
        } else {
            let _ = fs::remove_file(path);
            None
        }
    }
    #[cfg(not(unix))]
    {
        // Simple fallback for non-unix
        Some(pid)
    }
}

pub fn stop_server() -> Result<()> {
    if let Some(pid) = is_running() {
        println!("Stopping UniGateway (PID: {})...", pid);
        #[cfg(unix)]
        {
            let status = Command::new("kill")
                .arg(pid.to_string())
                .status()
                .context("failed to kill process")?;
            if !status.success() {
                bail!("failed to stop process {}", pid);
            }
        }
        #[cfg(not(unix))]
        {
            bail!("Stop command not yet supported on this platform. Please kill PID {} manually.", pid);
        }
        let _ = fs::remove_file(pid_path());
        println!("Stopped.");
    } else {
        println!("UniGateway is not running.");
    }
    Ok(())
}

pub fn status_server() -> Result<()> {
    if let Some(pid) = is_running() {
        println!("UniGateway is running (PID: {}).", pid);
        println!("Logs: {}", log_path().display());
    } else {
        println!("UniGateway is not running.");
    }
    Ok(())
}

pub fn view_logs(follow: bool) -> Result<()> {
    let path = log_path();
    if !path.exists() {
        println!("Log file not found: {}", path.display());
        return Ok(());
    }

    if follow {
        let mut child = Command::new("tail")
            .arg("-f")
            .arg(path)
            .spawn()
            .context("failed to spawn tail")?;
        let _ = child.wait();
    } else {
        let contents = fs::read_to_string(path)?;
        print!("{}", contents);
    }
    Ok(())
}

pub fn daemonize() -> Result<()> {
    let pid_file = pid_path();
    let log_file = log_path();

    if is_running().is_some() {
        println!("UniGateway is already running.");
        return Ok(());
    }

    let exe = std::env::current_exe()?;
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let mut args: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < raw_args.len() {
        match raw_args[i].as_str() {
            "--bind" | "-b" | "--config" | "-c" => {
                if i + 1 < raw_args.len() {
                    args.push(raw_args[i].clone());
                    args.push(raw_args[i + 1].clone());
                    i += 1;
                }
            }
            "--no-ui" => args.push(raw_args[i].clone()),
            _ => {}
        }
        i += 1;
    }

    let mut cmd = Command::new(exe);
    cmd.arg("serve");
    for arg in args {
        cmd.arg(arg);
    }
    cmd.arg("--detached");

    // Create log file if it doesn't exist
    let log_handle = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)?;

    let mut child = cmd
        .stdout(std::process::Stdio::from(log_handle.try_clone()?))
        .stderr(std::process::Stdio::from(log_handle))
        .spawn()
        .context("failed to spawn background process")?;

    let pid = child.id();
    std::thread::sleep(Duration::from_millis(150));
    if let Some(status) = child.try_wait().context("failed to check background process status")? {
        bail!(
            "failed to start background process (exit: {}). See logs: {}",
            status,
            log_file.display()
        );
    }

    fs::write(pid_file, pid.to_string())?;

    println!("UniGateway started in background (PID: {}).", pid);
    println!("Current config has been loaded.");
    println!("Logs: {}", log_file.display());
    println!("Stop with: ug stop");

    Ok(())
}
