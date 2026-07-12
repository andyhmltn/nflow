use std::fs::OpenOptions;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub fn config_dir() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME not set");
    PathBuf::from(home).join(".config").join("nflow")
}

pub fn pid_path() -> PathBuf {
    config_dir().join("nflow.pid")
}

pub fn log_path() -> PathBuf {
    config_dir().join("nflow.log")
}

fn read_pid() -> Option<i32> {
    let contents = std::fs::read_to_string(pid_path()).ok()?;
    contents.trim().parse::<i32>().ok()
}

pub fn process_alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

pub fn is_running() -> Option<i32> {
    let pid = read_pid()?;
    if process_alive(pid) {
        Some(pid)
    } else {
        None
    }
}

pub fn write_pid(pid: i32) -> std::io::Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    std::fs::write(pid_path(), pid.to_string())
}

pub fn start() {
    if let Some(pid) = is_running() {
        println!("nflow already running (pid {pid})");
        return;
    }

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("failed to resolve executable path: {e}");
            std::process::exit(1);
        }
    };

    let dir = config_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("failed to create config directory: {e}");
        std::process::exit(1);
    }

    let log = match OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("failed to open log file: {e}");
            std::process::exit(1);
        }
    };
    let log_err = match log.try_clone() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("failed to open log file: {e}");
            std::process::exit(1);
        }
    };

    let mut command = Command::new(exe);
    command
        .arg("run")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));

    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }

    match command.spawn() {
        Ok(child) => {
            let pid = child.id() as i32;
            if let Err(e) = write_pid(pid) {
                eprintln!("failed to write pid file: {e}");
            }
            println!("nflow started (pid {pid}) -- menu bar icon active");
        }
        Err(e) => {
            eprintln!("failed to start nflow: {e}");
            std::process::exit(1);
        }
    }
}

pub fn stop() {
    match read_pid() {
        Some(pid) if process_alive(pid) => {
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
            let _ = std::fs::remove_file(pid_path());
            println!("nflow stopped (pid {pid})");
        }
        _ => {
            let _ = std::fs::remove_file(pid_path());
            println!("nflow is not running");
        }
    }
}

pub fn status() {
    match is_running() {
        Some(pid) => println!("nflow is running (pid {pid})"),
        None => println!("nflow is not running"),
    }
}
