//! In-game updater (main menu "Check for updates"). Downloads only what changed since the version running:
//! - assets (and README/CHANGELOG/LICENSE): every one is in git, byte for byte what the release bundles hold, so the
//!   git tree of the release tag (GitHub API) gives each file's blob hash; files whose local hash differs are fetched
//!   one by one from raw.githubusercontent.com. Files the old version shipped and the new one does not are deleted
//!   (only if unmodified); anything else in `assets/` (maps saved by the editor) is left alone;
//! - the executable: the release holds `longrange-<v>-<platform>-exe.zst` (the whole exe) and
//!   `longrange-<v>-<platform>-from-<previous v>.zst` (a zstd `--patch-from` diff against the previous release's exe),
//!   made by `scripts/exe_updates.sh`. The patch is tried when the running version is the previous one; the zstd
//!   frame checksum proves the result, otherwise the whole exe is fetched.
//! Downloads go through the `curl` program (Windows 10+ and every Linux have it) into `.update/` next to the exe,
//! are checked, then moved into place; the running exe is renamed to `*.old` (removed at the next start by `cleanup`).
//! Only for an installed release: `cargo run` and `LR_ASSETS` turn it off.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

pub const REPO: &str = "Monk1mmmmm/long-range";
pub const PLATFORM: &str = if cfg!(windows) { "windows-x86_64" } else { "linux-x86_64" };
const STAGING: &str = ".update";

#[derive(Clone)]
pub struct FileGet {
    /// Relative to the install folder, with '/'.
    path: String,
    sha: String,
    size: u64,
}

#[derive(Clone)]
pub struct Plan {
    pub tag: String,
    /// The release notes of every version newer than the running one, newest first.
    pub notes: String,
    pub files: usize,
    pub removed: usize,
    /// Expected download size: changed files + the exe patch (or the whole exe when no patch fits).
    pub bytes: u64,
    get: Vec<FileGet>,
    remove: Vec<String>,
    patch: Option<(String, u64)>,
    full: (String, u64),
}

#[derive(Clone)]
pub enum State {
    Busy(String),
    UpToDate,
    Ready(Plan),
    /// Installed: restart to run it.
    Done(String),
    Failed(String),
}

pub struct Updater {
    state: Arc<Mutex<State>>,
}

impl Updater {
    /// Starts looking for a newer release in the background.
    pub fn check() -> Updater {
        let u = Updater { state: Arc::new(Mutex::new(State::Busy(tr!("update.checking").into()))) };
        let s = u.state.clone();
        std::thread::spawn(move || {
            let r = plan(&s);
            set(&s, match r {
                Ok(Some(p)) => State::Ready(p),
                Ok(None) => State::UpToDate,
                Err(e) => State::Failed(e),
            });
        });
        u
    }

    pub fn state(&self) -> State {
        self.state.lock().unwrap().clone()
    }

    pub fn busy(&self) -> bool {
        matches!(self.state(), State::Busy(_))
    }

    /// Downloads and installs a plan from `check` in the background.
    pub fn install(&self, p: Plan) {
        let s = self.state.clone();
        set(&s, State::Busy(tr!("update.downloading").into()));
        std::thread::spawn(move || {
            let tag = p.tag.clone();
            set(&s, match install(&s, p) {
                Ok(()) => State::Done(tag),
                Err(e) => State::Failed(e),
            });
        });
    }
}

fn set(s: &Mutex<State>, v: State) {
    *s.lock().unwrap() = v;
}

/// The folder the release was unpacked into, or why updating is not possible here.
pub fn install_dir() -> Result<PathBuf, String> {
    if std::env::var_os("CARGO_MANIFEST_DIR").is_some() || std::env::var_os("LR_ASSETS").is_some() {
        return Err(tr!("update.dev_build").into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("no exe folder")?.to_path_buf();
    if !dir.join("assets").is_dir() {
        return Err(tr!("update.dev_build").into());
    }
    Ok(dir)
}

/// Removes what the last update left: the replaced exe.
pub fn cleanup() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(old_exe(&exe));
    }
}

/// Starts the (updated) game again; the caller then quits.
pub fn restart() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Command::new(exe).spawn().map(|_| ()).map_err(|e| e.to_string())
}

fn old_exe(exe: &Path) -> PathBuf {
    exe.with_extension("old")
}

fn version(s: &str) -> Vec<u32> {
    s.trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

fn curl() -> Command {
    let mut c = Command::new("curl");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashing up
    }
    c.args(["-fsSL", "--retry", "2", "--connect-timeout", "20", "-A", "longrange-updater"]);
    c
}

fn run(mut c: Command) -> Result<Vec<u8>, String> {
    let out = c.output().map_err(|e| tr!("update.no_curl", e))?;
    if !out.status.success() {
        return Err(tr!("update.download_failed", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(out.stdout)
}

fn api(path: &str) -> Result<serde_json::Value, String> {
    let mut c = curl();
    c.args(["-H", "Accept: application/vnd.github+json"]).arg(format!("https://api.github.com/repos/{REPO}/{path}"));
    let b = run(c)?;
    serde_json::from_slice(&b).map_err(|e| e.to_string())
}

/// Files of the release a tag names that the updater keeps current: path -> (blob hash, size).
fn tree(tag: &str) -> Result<Vec<FileGet>, String> {
    let t = api(&format!("git/trees/{tag}?recursive=1"))?;
    if t["truncated"].as_bool() == Some(true) {
        return Err("git tree truncated".into());
    }
    let mut v = Vec::new();
    for e in t["tree"].as_array().ok_or("no git tree")? {
        let path = e["path"].as_str().unwrap_or("");
        if e["type"] != "blob" || !(path.starts_with("assets/") || matches!(path, "README.md" | "CHANGELOG.md" | "LICENSE")) {
            continue;
        }
        v.push(FileGet { path: path.into(), sha: e["sha"].as_str().unwrap_or("").into(), size: e["size"].as_u64().unwrap_or(0) });
    }
    Ok(v)
}

/// Git's blob hash of a file (sha1 of "blob <len>\0" + content), or None when it cannot be read.
fn blob_sha(p: &Path) -> Option<String> {
    let b = std::fs::read(p).ok()?;
    let mut h = sha1_smol::Sha1::new();
    h.update(format!("blob {}\0", b.len()).as_bytes());
    h.update(&b);
    Some(h.digest().to_string())
}

fn same(p: &Path, f: &FileGet) -> bool {
    std::fs::metadata(p).map_or(false, |m| m.len() == f.size) && blob_sha(p).as_deref() == Some(&f.sha)
}

fn plan(s: &Mutex<State>) -> Result<Option<Plan>, String> {
    let dir = install_dir()?;
    let cur = env!("CARGO_PKG_VERSION");
    let rel = api("releases?per_page=30")?;
    let mut newer: Vec<&serde_json::Value> = rel
        .as_array()
        .ok_or("no releases")?
        .iter()
        .filter(|r| r["draft"] != true && r["prerelease"] != true && version(r["tag_name"].as_str().unwrap_or("")) > version(cur))
        .collect();
    newer.sort_by_key(|r| std::cmp::Reverse(version(r["tag_name"].as_str().unwrap_or(""))));
    let Some(latest) = newer.first() else { return Ok(None) };
    let tag = latest["tag_name"].as_str().unwrap_or("").to_string();
    let ver = tag.trim_start_matches('v').to_string();
    let notes = newer.iter().map(|r| format!("{}\n{}", r["tag_name"].as_str().unwrap_or(""), r["body"].as_str().unwrap_or("").trim())).collect::<Vec<_>>().join("\n\n");
    let asset = |name: &str| {
        latest["assets"].as_array().and_then(|a| a.iter().find(|x| x["name"] == name)).map(|x| (x["browser_download_url"].as_str().unwrap_or("").to_string(), x["size"].as_u64().unwrap_or(0)))
    };
    let full = asset(&format!("longrange-{ver}-{PLATFORM}-exe.zst")).ok_or_else(|| tr!("update.no_files", tag))?;
    let patch = asset(&format!("longrange-{ver}-{PLATFORM}-from-{cur}.zst"));

    set(s, State::Busy(tr!("update.comparing").into()));
    let new = tree(&tag)?;
    let get: Vec<FileGet> = new.iter().filter(|f| !same(&dir.join(&f.path), f)).cloned().collect();
    // Deleted in the new version: only what the running version shipped, and only if nobody changed it.
    let mut remove = Vec::new();
    if let Ok(old) = tree(&format!("v{cur}")) {
        let keep: std::collections::HashSet<&str> = new.iter().map(|f| f.path.as_str()).collect();
        for f in old.iter().filter(|f| !keep.contains(f.path.as_str())) {
            if same(&dir.join(&f.path), f) {
                remove.push(f.path.clone());
            }
        }
    }
    let exe_bytes = patch.as_ref().map_or(full.1, |p| p.1);
    let bytes = get.iter().map(|f| f.size).sum::<u64>() + exe_bytes;
    Ok(Some(Plan { tag, notes, files: get.len(), removed: remove.len(), bytes, get, remove, patch, full }))
}

/// Percent-encodes a repository path for a URL ('/' stays).
fn url_path(p: &str) -> String {
    let mut o = String::new();
    for b in p.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

/// Runs curl in `cwd`, reporting the bytes of `watch` (paths in `cwd`) written so far against `total`.
fn download(s: &Mutex<State>, cwd: &Path, args: &[String], watch: &[(String, u64)], done: u64, total: u64) -> Result<(), String> {
    let attempt = |parallel: bool| -> Result<std::process::ExitStatus, String> {
        let mut c = curl();
        c.current_dir(cwd).args(["--create-dirs", "--compressed"]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
        if parallel {
            c.args(["--parallel", "--parallel-max", "6"]);
        }
        let mut ch = c.args(args).spawn().map_err(|e| tr!("update.no_curl", e))?;
        loop {
            if let Some(st) = ch.try_wait().map_err(|e| e.to_string())? {
                if !st.success() && st.code() != Some(2) {
                    let mut err = String::new();
                    if let Some(mut e) = ch.stderr.take() {
                        let _ = std::io::Read::read_to_string(&mut e, &mut err);
                    }
                    return Err(tr!("update.download_failed", err.trim()));
                }
                return Ok(st);
            }
            let got: u64 = watch.iter().map(|(p, n)| std::fs::metadata(cwd.join(p)).map_or(0, |m| m.len().min(*n))).sum();
            set(s, State::Busy(tr!("update.progress", mb(done + got), mb(total))));
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    };
    // Exit code 2: this curl does not know --parallel (old Windows 10 builds ship 7.55): one at a time.
    if attempt(true)?.code() == Some(2) {
        attempt(false)?;
    }
    Ok(())
}

pub fn mb(b: u64) -> String {
    format!("{:.1}", b as f64 / 1_048_576.0)
}

fn unzstd(data: &[u8], prefix: Option<&[u8]>) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    match prefix {
        Some(p) => {
            let mut d = zstd::stream::read::Decoder::with_ref_prefix(data, p)?;
            d.window_log_max(31)?;
            std::io::copy(&mut d, &mut out)?;
        }
        None => {
            let mut d = zstd::stream::read::Decoder::new(data)?;
            d.window_log_max(31)?;
            std::io::copy(&mut d, &mut out)?;
        }
    }
    Ok(out)
}

fn install(s: &Mutex<State>, p: Plan) -> Result<(), String> {
    let dir = install_dir()?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let stage = dir.join(STAGING);
    std::fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
    let io = |e: std::io::Error| e.to_string();

    // Files left from an interrupted attempt that are already right are not fetched again.
    let todo: Vec<&FileGet> = p.get.iter().filter(|f| !same(&stage.join(&f.path), f)).collect();
    let total = todo.iter().map(|f| f.size).sum::<u64>() + p.patch.as_ref().map_or(p.full.1, |x| x.1);
    if !todo.is_empty() {
        let mut cfg = String::new();
        for f in &todo {
            let q = |x: &str| x.replace('\\', "\\\\").replace('"', "\\\"");
            cfg += &format!("url = \"https://raw.githubusercontent.com/{REPO}/{}/{}\"\noutput = \"{}\"\n", p.tag, url_path(&f.path), q(&f.path));
        }
        std::fs::write(stage.join("files.txt"), cfg).map_err(io)?;
        let watch: Vec<(String, u64)> = todo.iter().map(|f| (f.path.clone(), f.size)).collect();
        download(s, &stage, &["-K".into(), "files.txt".into()], &watch, 0, total)?;
        for f in &todo {
            if !same(&stage.join(&f.path), f) {
                return Err(tr!("update.bad_file", f.path));
            }
        }
    }
    let done = todo.iter().map(|f| f.size).sum::<u64>();

    // The executable: the patch against the running exe if there is one, else (or if it does not fit) the whole.
    let mut new_exe = None;
    if let Some((url, n)) = &p.patch {
        download(s, &stage, &["-o".into(), "exe.patch".into(), url.clone()], &[("exe.patch".into(), *n)], done, total)?;
        set(s, State::Busy(tr!("update.patching").into()));
        let cur = std::fs::read(&exe).map_err(io)?;
        let d = std::fs::read(stage.join("exe.patch")).map_err(io)?;
        new_exe = unzstd(&d, Some(&cur)).ok();
    }
    let new_exe = match new_exe {
        Some(b) => b,
        None => {
            let (url, n) = &p.full;
            download(s, &stage, &["-o".into(), "exe.zst".into(), url.clone()], &[("exe.zst".into(), *n)], done, total + n)?;
            set(s, State::Busy(tr!("update.patching").into()));
            let d = std::fs::read(stage.join("exe.zst")).map_err(io)?;
            unzstd(&d, None).map_err(|e| tr!("update.bad_file", e))?
        }
    };
    let staged_exe = stage.join("exe.new");
    std::fs::write(&staged_exe, &new_exe).map_err(io)?;

    // Everything is here and checked: move it into place.
    set(s, State::Busy(tr!("update.installing").into()));
    for f in &p.get {
        let to = dir.join(&f.path);
        if let Some(d) = to.parent() {
            std::fs::create_dir_all(d).map_err(io)?;
        }
        std::fs::rename(stage.join(&f.path), &to).map_err(|e| format!("{}: {e}", f.path))?;
    }
    for r in &p.remove {
        let _ = std::fs::remove_file(dir.join(r));
    }
    let old = old_exe(&exe);
    let _ = std::fs::remove_file(&old);
    // Windows lets a running exe be renamed, not overwritten.
    std::fs::rename(&exe, &old).map_err(io)?;
    if let Err(e) = std::fs::rename(&staged_exe, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(e.to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755));
    }
    let _ = std::fs::remove_dir_all(&stage);
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn blob_hash_matches_git() {
        // `git hash-object` of "hello\n".
        let p = std::env::temp_dir().join(format!("lr-blob-{}", std::process::id()));
        std::fs::write(&p, "hello\n").unwrap();
        assert_eq!(super::blob_sha(&p).unwrap(), "ce013625030ba8dba906f756967f9e9ca394464a");
        let _ = std::fs::remove_file(p);
        assert!(super::version("v0.16.10") > super::version("0.16.9"));
        assert_eq!(super::url_path("assets/a b+c.png"), "assets/a%20b%2Bc.png");
    }

    #[test]
    fn patch_round_trip() {
        // What `zstd --patch-from=old new` writes: the old file as a ref prefix, with a content checksum.
        let old: Vec<u8> = (0..400_000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
        let mut new = old.clone();
        new[1000..1100].fill(7);
        new.extend_from_slice(b"more code");
        let mut e = zstd::stream::write::Encoder::with_ref_prefix(Vec::new(), 19, &old).unwrap();
        e.include_checksum(true).unwrap();
        e.long_distance_matching(true).unwrap();
        e.window_log(27).unwrap();
        std::io::Write::write_all(&mut e, &new).unwrap();
        let patch = e.finish().unwrap();
        assert!(patch.len() < 2000, "patch {} bytes", patch.len());
        assert_eq!(super::unzstd(&patch, Some(&old)).unwrap(), new);
        // Applied to another exe it fails (checksum), so the updater fetches the whole exe.
        let mut other = old.clone();
        other[5000] ^= 1;
        assert!(super::unzstd(&patch, Some(&other)).map_or(true, |b| b != new));
        assert!(super::unzstd(&patch, Some(&other)).is_err());
    }
}
