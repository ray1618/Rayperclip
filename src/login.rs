//! "Start at login" support via a per-user LaunchAgent.

use std::path::{Path, PathBuf};

/// File name of the LaunchAgent plist.
pub fn plist_name() -> String {
    format!("{}.plist", crate::APP_ID)
}

/// `~/Library/LaunchAgents`.
pub fn launch_agents_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.home_dir().join("Library/LaunchAgents"))
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Render the LaunchAgent property list that starts `executable` at login.
pub fn plist_contents(label: &str, executable: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#,
        label = xml_escape(label),
        exe = xml_escape(&executable.to_string_lossy()),
    )
}

/// Install the LaunchAgent in `agents_dir`.
pub fn enable(agents_dir: &Path, executable: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(agents_dir)?;
    let path = agents_dir.join(plist_name());
    std::fs::write(&path, plist_contents(crate::APP_ID, executable))?;
    Ok(path)
}

/// Remove the LaunchAgent from `agents_dir` (no error when absent).
pub fn disable(agents_dir: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(agents_dir.join(plist_name())) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

pub fn is_enabled(agents_dir: &Path) -> bool {
    agents_dir.join(plist_name()).is_file()
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Program path stored in an installed LaunchAgent, if any.
pub fn program_path(agents_dir: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(agents_dir.join(plist_name())).ok()?;
    let after = &text[text.find("<key>ProgramArguments</key>")?..];
    let start = after.find("<string>")? + "<string>".len();
    let end = start + after[start..].find("</string>")?;
    Some(PathBuf::from(xml_unescape(&after[start..end])))
}

/// Whether the LaunchAgent must be (re)written or removed so it matches the
/// desired state. An enabled agent pointing at another executable (e.g. an
/// old build after moving the app) needs to be rewritten.
pub fn needs_update(agents_dir: &Path, executable: &Path, enabled: bool) -> bool {
    match (is_enabled(agents_dir), enabled) {
        (false, false) => false,
        (true, false) | (false, true) => true,
        (true, true) => program_path(agents_dir).as_deref() != Some(executable),
    }
}

/// Enable or disable as requested.
pub fn apply(agents_dir: &Path, executable: &Path, enabled: bool) -> std::io::Result<()> {
    if enabled {
        enable(agents_dir, executable).map(|_| ())
    } else {
        disable(agents_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_contains_label_program_and_run_at_load() {
        let p = plist_contents("nl.ray.rayperclip", Path::new("/Applications/R.app/R"));
        assert!(p.contains("<string>nl.ray.rayperclip</string>"));
        assert!(p.contains("<string>/Applications/R.app/R</string>"));
        assert!(p.contains("<key>RunAtLoad</key>\n    <true/>"));
        assert!(p.starts_with("<?xml"));
    }

    #[test]
    fn plist_escapes_special_characters() {
        let p = plist_contents("a&b", Path::new("/tmp/<x> & 'y'"));
        assert!(p.contains("a&amp;b"));
        assert!(p.contains("/tmp/&lt;x&gt; &amp; &apos;y&apos;"));
    }

    #[test]
    fn enable_disable_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let agents = dir.path().join("LaunchAgents");
        assert!(!is_enabled(&agents));
        let path = enable(&agents, Path::new("/bin/rayperclip")).unwrap();
        assert!(path.ends_with("nl.ray.rayperclip.plist"));
        assert!(is_enabled(&agents));
        disable(&agents).unwrap();
        assert!(!is_enabled(&agents));
        disable(&agents).unwrap(); // idempotent
    }

    #[test]
    fn program_path_reads_installed_agent() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(program_path(dir.path()), None);
        let exe = Path::new("/Users/me/Applications/R & D.app/Contents/MacOS/R");
        enable(dir.path(), exe).unwrap();
        assert_eq!(program_path(dir.path()).as_deref(), Some(exe));
    }

    #[test]
    fn stale_program_path_needs_update() {
        let dir = tempfile::tempdir().unwrap();
        let old = Path::new("/repo/target/release/rayperclip");
        let new = Path::new("/Users/me/Applications/Rayperclip.app/Contents/MacOS/Rayperclip");
        enable(dir.path(), old).unwrap();
        assert!(needs_update(dir.path(), new, true));
        apply(dir.path(), new, true).unwrap();
        assert!(!needs_update(dir.path(), new, true));
        assert_eq!(program_path(dir.path()).as_deref(), Some(new));
    }

    #[test]
    fn needs_update_for_on_off_changes() {
        let dir = tempfile::tempdir().unwrap();
        let exe = Path::new("/x");
        assert!(!needs_update(dir.path(), exe, false));
        assert!(needs_update(dir.path(), exe, true));
        enable(dir.path(), exe).unwrap();
        assert!(!needs_update(dir.path(), exe, true));
        assert!(needs_update(dir.path(), exe, false));
    }

    #[test]
    fn apply_toggles() {
        let dir = tempfile::tempdir().unwrap();
        apply(dir.path(), Path::new("/x"), true).unwrap();
        assert!(is_enabled(dir.path()));
        apply(dir.path(), Path::new("/x"), false).unwrap();
        assert!(!is_enabled(dir.path()));
    }
}
