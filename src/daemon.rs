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

fn launch_agent_path() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME not set");
    PathBuf::from(home)
        .join("Library")
        .join("LaunchAgents")
        .join("com.nflow.plist")
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn launch_agent_plist(executable: &std::path::Path) -> String {
    let executable = xml_escape(&executable.display().to_string());
    let log = xml_escape(&log_path().display().to_string());
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n    <key>Label</key>\n    <string>com.nflow</string>\n    <key>ProgramArguments</key>\n    <array>\n        <string>{executable}</string>\n        <string>run</string>\n    </array>\n    <key>RunAtLoad</key>\n    <true/>\n    <key>StandardOutPath</key>\n    <string>{log}</string>\n    <key>StandardErrorPath</key>\n    <string>{log}</string>\n</dict>\n</plist>\n"
    )
}

pub fn enable_autostart() {
    let executable = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("failed to resolve executable path: {error}");
            std::process::exit(1);
        }
    };
    let path = launch_agent_path();
    let parent = path.parent().expect("launch agent path must have a parent");

    if let Err(error) = std::fs::create_dir_all(parent) {
        eprintln!("failed to create LaunchAgents directory: {error}");
        std::process::exit(1);
    }
    if let Err(error) = std::fs::write(&path, launch_agent_plist(&executable)) {
        eprintln!("failed to write launch agent: {error}");
        std::process::exit(1);
    }

    println!(
        "nflow will start automatically when you next log in ({})",
        path.display()
    );
}

pub fn disable_autostart() {
    let path = launch_agent_path();
    match std::fs::remove_file(&path) {
        Ok(()) => println!("nflow automatic startup disabled"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            println!("nflow automatic startup is not enabled")
        }
        Err(error) => {
            eprintln!("failed to remove launch agent: {error}");
            std::process::exit(1);
        }
    }
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

#[cfg(test)]
mod tests {
    use super::{launch_agent_plist, xml_escape};
    use std::path::Path;

    #[test]
    fn escapes_xml_values() {
        assert_eq!(xml_escape("a&<>'\""), "a&amp;&lt;&gt;&apos;&quot;");
    }

    #[test]
    fn launch_agent_runs_the_executable_in_daemon_mode() {
        let plist = launch_agent_plist(Path::new("/Applications/nflow & tools"));

        assert!(plist.contains("<string>/Applications/nflow &amp; tools</string>"));
        assert!(plist.contains("<string>run</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
    }
}
