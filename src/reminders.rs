//! Daily study reminder: a tiny background helper (`lumi --reminders`) started at login.
//! At most one notification a day, at the chosen time, only if the learner hasn't studied yet.

use crate::db::Db;
use crate::game;
use chrono::{Local, Timelike};
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub fn notify(title: &str, body: &str) {
    #[cfg(target_os = "linux")]
    let _ = Command::new("notify-send").args(["-a", "Lumi", title, body]).status();
    #[cfg(target_os = "macos")]
    let _ = Command::new("osascript").args(["-e", &format!("display notification {body:?} with title {title:?}")]).status();
    #[cfg(target_os = "windows")]
    {
        let esc = |s: &str| s.replace('\'', "''");
        let ps = format!(
            "Add-Type -AssemblyName System.Windows.Forms; $n = New-Object System.Windows.Forms.NotifyIcon; \
             $n.Icon = [System.Drawing.SystemIcons]::Information; $n.Visible = $true; \
             $n.ShowBalloonTip(10000, '{}', '{}', 'Info'); Start-Sleep 11; $n.Dispose()",
            esc(title), esc(body)
        );
        use std::os::windows::process::CommandExt;
        let _ = Command::new("powershell").args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps]).creation_flags(0x08000000).spawn();
    }
}

pub fn message(db: &Db) -> (String, String) {
    let s = game::streak(db);
    let hi = db.get::<String>("name").filter(|n| !n.is_empty()).map(|n| format!("{n}, ")).unwrap_or_default();
    let due = game::due_concepts(db, None).len();
    if s >= 2 {
        (format!("🔥 Your {s}-day streak is on the line!"), format!("{hi}a 5-minute session keeps it alive."))
    } else if due > 0 {
        ("🧠 Time for a quick review".into(), format!("{hi}{due} concept{} ready to lock into memory.", if due > 1 { "s are" } else { " is" }))
    } else {
        ("✨ Lumi misses you".into(), format!("{hi}a few minutes today adds up fast. Open Lumi to keep learning."))
    }
}

pub fn should_remind(db: &Db) -> bool {
    if !db.get_or("reminders", false) {
        return false;
    }
    let now = Local::now();
    let t: String = db.get_or("reminder_time", "19:00".to_string());
    let (h, m) = t.split_once(':').and_then(|(h, m)| Some((h.parse::<u32>().ok()?, m.parse::<u32>().ok()?))).unwrap_or((19, 0));
    if (now.hour(), now.minute()) < (h, m) {
        return false;
    }
    if db.get::<String>("last_reminder").as_deref() == Some(game::today().as_str()) {
        return false;
    }
    game::today_xp(db) == 0
}

/// Loop until reminders are switched off. Single instance via a PID file.
pub fn run_daemon(db: &Db) {
    let pid_file = db.path("reminders.pid");
    if let Ok(pid) = std::fs::read_to_string(&pid_file) {
        if process_alive(pid.trim().parse().unwrap_or(0)) {
            return;
        }
    }
    let _ = std::fs::write(&pid_file, std::process::id().to_string());
    while db.get_or("reminders", false) {
        if should_remind(db) {
            db.set("last_reminder", &game::today());
            let (t, b) = message(db);
            notify(&t, &b);
        }
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
    let _ = std::fs::remove_file(pid_file);
}

fn process_alive(pid: u32) -> bool {
    if pid == 0 || pid == std::process::id() {
        return false;
    }
    #[cfg(target_os = "linux")]
    return std::path::Path::new(&format!("/proc/{pid}")).exists();
    #[cfg(not(target_os = "linux"))]
    return false;
}

fn autostart_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    return dirs::data_dir().map(|d| d.join("Microsoft\\Windows\\Start Menu\\Programs\\Startup\\lumi-reminders.bat"));
    #[cfg(target_os = "macos")]
    return dirs::home_dir().map(|d| d.join("Library/LaunchAgents/app.lumi.reminders.plist"));
    #[cfg(all(unix, not(target_os = "macos")))]
    return dirs::config_dir().map(|d| d.join("autostart/lumi-reminders.desktop"));
}

/// Install/remove the login item and start the helper now.
pub fn configure(enabled: bool) {
    if std::env::var("LUMI_NO_AUTOSTART").is_ok() {
        return;
    }
    let (Some(path), Ok(exe)) = (autostart_path(), std::env::current_exe()) else { return };
    if !enabled {
        let _ = std::fs::remove_file(path);
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let exe_s = exe.to_string_lossy();
    #[cfg(target_os = "windows")]
    let content = format!("@echo off\r\nstart \"\" \"{exe_s}\" --reminders\r\n");
    #[cfg(target_os = "macos")]
    let content = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict><key>Label</key><string>app.lumi.reminders</string><key>ProgramArguments</key><array><string>{exe_s}</string><string>--reminders</string></array><key>RunAtLoad</key><true/></dict></plist>");
    #[cfg(all(unix, not(target_os = "macos")))]
    let content = format!("[Desktop Entry]\nType=Application\nName=Lumi reminders\nComment=Daily study reminder from Lumi\nExec=\"{exe_s}\" --reminders\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n");
    let _ = std::fs::write(&path, content);
    let _ = Command::new(exe).arg("--reminders").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}
