//! Remote access over SSH without libssh2.
//!
//! `git2` is built without its `ssh` feature, so libgit2 has no transport of
//! its own for `ssh://` URLs. This module registers one that does what the git
//! command line does: spawn the system `ssh` binary running `git-upload-pack`
//! or `git-receive-pack` on the remote machine and speak the smart protocol
//! over its stdin/stdout. The user's `~/.ssh/config`, keys and agent apply
//! unchanged, which is exactly what "sync with my laptop over SSH" needs.
//!
//! `ssh` runs in batch mode, so a host that would ask for a password fails
//! immediately with a clear error instead of hanging on a prompt.
//!
//! The same mechanism serves `file://` URLs by running `git upload-pack` /
//! `git receive-pack` locally. libgit2's built-in local transport cannot push
//! into a non-bare repository, which is exactly what another Piki notes
//! directory on the same machine (or a mounted drive) is.

use git2::transport::{Service, SmartSubtransport, SmartSubtransportStream, Transport};
use std::io::{self, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex, Once, RwLock};
use std::time::{Duration, Instant};

/// Default location of the notes directory on a remote machine, relative to
/// the remote user's home directory.
pub const DEFAULT_REMOTE_PATH: &str = "~/.piki";

/// Seconds `ssh` waits for a connection before giving up.
const CONNECT_TIMEOUT_SECS: u32 = 15;

/// Exit status `ssh` uses for its own failures (connection refused, host key
/// problems, authentication failure) as opposed to the remote command failing.
const SSH_EXIT_FAILURE: i32 = 255;

static SSH_PROGRAM_OVERRIDE: RwLock<Option<String>> = RwLock::new(None);

/// Use `program` instead of `ssh` for every remote connection made by this
/// process. Tests use this to substitute a script that runs the remote command
/// locally; `PIKI_SSH` / `GIT_SSH` in the environment do the same for users.
pub fn set_ssh_program(program: Option<&str>) {
    *SSH_PROGRAM_OVERRIDE.write().unwrap() = program.map(str::to_string);
}

fn ssh_program() -> String {
    if let Some(p) = SSH_PROGRAM_OVERRIDE.read().unwrap().as_ref() {
        return p.clone();
    }
    std::env::var("PIKI_SSH")
        .or_else(|_| std::env::var("GIT_SSH"))
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "ssh".to_string())
}

/// The parts of an `ssh://[user@]host[:port]/path` URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshUrl {
    pub user: Option<String>,
    pub host: String,
    pub port: Option<u16>,
    /// Path on the remote machine. `ssh://host/~/notes` yields `~/notes`, which
    /// `git-upload-pack` expands relative to the remote user's home.
    pub path: String,
}

impl SshUrl {
    /// Parse an `ssh://` URL. Anything else (including scp-style `host:path`,
    /// see [`normalize_url`]) is rejected.
    pub fn parse(url: &str) -> Result<SshUrl, String> {
        let rest = url
            .strip_prefix("ssh://")
            .ok_or_else(|| format!("Not an ssh:// URL: {url}"))?;
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, ""),
        };
        if authority.is_empty() {
            return Err(format!("Missing host name in URL: {url}"));
        }
        let (user, hostport) = match authority.rsplit_once('@') {
            Some((u, h)) => (Some(u.to_string()), h),
            None => (None, authority),
        };
        let (host, port) = match hostport.rsplit_once(':') {
            Some((h, p)) if !p.is_empty() => {
                let port = p
                    .parse::<u16>()
                    .map_err(|_| format!("Invalid port '{p}' in URL: {url}"))?;
                (h, Some(port))
            }
            _ => (hostport, None),
        };
        if host.is_empty() {
            return Err(format!("Missing host name in URL: {url}"));
        }
        // `/~/x` and `/~user/x` are git's spelling of home-relative paths.
        let path = match path.strip_prefix("/~") {
            Some(rest) => format!("~{rest}"),
            None if path.is_empty() => "/".to_string(),
            None => path.to_string(),
        };
        Ok(SshUrl {
            user,
            host: host.to_string(),
            port,
            path,
        })
    }

    /// `user@host` or plain `host`, as passed to `ssh`.
    pub fn destination(&self) -> String {
        match &self.user {
            Some(u) => format!("{u}@{}", self.host),
            None => self.host.clone(),
        }
    }

    /// The URL in canonical `ssh://` form.
    pub fn to_url(&self) -> String {
        let mut s = format!("ssh://{}", self.destination());
        if let Some(p) = self.port {
            s.push_str(&format!(":{p}"));
        }
        if let Some(rest) = self.path.strip_prefix('~') {
            s.push_str("/~");
            s.push_str(rest);
        } else {
            s.push_str(&self.path);
        }
        s
    }

    /// An `ssh` command line addressing this host, without a remote command.
    fn command(&self) -> Command {
        let mut cmd = Command::new(ssh_program());
        cmd.arg("-o")
            .arg("BatchMode=yes")
            .arg("-o")
            .arg(format!("ConnectTimeout={CONNECT_TIMEOUT_SECS}"));
        if let Some(p) = self.port {
            cmd.arg("-p").arg(p.to_string());
        }
        cmd.arg("--").arg(self.destination());
        cmd
    }
}

/// URL for the notes directory at `path` (default `~/.piki`) on `host`, which
/// may be a bare host name, `user@host`, or `user@host:port`.
pub fn url_for_host(host: &str, path: Option<&str>) -> Result<String, String> {
    let host = host.trim();
    if host.is_empty() || host.contains('/') || host.contains(char::is_whitespace) {
        return Err(format!("'{host}' is not a valid host name"));
    }
    let path = path.unwrap_or(DEFAULT_REMOTE_PATH);
    let path = path
        .strip_prefix('~')
        .map(|p| format!("/~{p}"))
        .unwrap_or_else(|| {
            if path.starts_with('/') {
                path.to_string()
            } else {
                format!("/~/{path}")
            }
        });
    let url = format!("ssh://{host}{path}");
    // Round-trip through the parser to validate.
    SshUrl::parse(&url).map(|u| u.to_url())
}

/// Turn the URL forms people type into something libgit2 can dispatch on. The
/// scp-like `host:path` and `user@host:path` are rewritten to `ssh://`, with a
/// relative path made home-relative as git does; absolute local paths become
/// `file://` URLs. Everything else is returned unchanged.
pub fn normalize_url(input: &str) -> String {
    let input = input.trim();
    if input.contains("://") {
        return input.to_string();
    }
    if is_absolute_local_path(input) {
        return file_url_for_path(input);
    }
    // A Windows drive letter (`C:\...`) is a local path, not a host.
    if let Some((host, path)) = input.split_once(':')
        && !host.is_empty()
        && !host.contains('/')
        && !host.contains('\\')
        && !(host.len() == 1 && host.chars().all(|c| c.is_ascii_alphabetic()))
        && !path.is_empty()
    {
        let path = if path.starts_with('/') {
            path.to_string()
        } else if let Some(rest) = path.strip_prefix('~') {
            format!("/~{rest}")
        } else {
            format!("/~/{path}")
        };
        return format!("ssh://{host}{path}");
    }
    input.to_string()
}

/// Is this a URL our SSH transport handles?
pub fn is_ssh_url(url: &str) -> bool {
    url.starts_with("ssh://")
}

/// Is this a `file://` URL (handled by the same transport, running git
/// locally)?
pub fn is_file_url(url: &str) -> bool {
    url.starts_with("file://")
}

/// The local path named by a `file://` URL.
fn file_url_path(url: &str) -> Option<String> {
    let rest = url.strip_prefix("file://")?;
    // `file:///C:/x` → `C:/x`; `file:///tmp/x` → `/tmp/x`.
    let bytes = rest.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
        return Some(rest[1..].to_string());
    }
    Some(rest.to_string())
}

/// `file://` URL for an absolute local path.
fn file_url_for_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
    }
}

fn is_absolute_local_path(s: &str) -> bool {
    s.starts_with('/')
        || (s.len() >= 3
            && s.as_bytes()[0].is_ascii_alphabetic()
            && s.as_bytes()[1] == b':'
            && matches!(s.as_bytes()[2], b'/' | b'\\'))
}

/// Quote `s` for a POSIX shell.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Quote a remote path for use in a shell command line, keeping a leading `~/`
/// outside the quotes so the remote shell still expands it.
pub fn shell_quote_path(path: &str) -> String {
    match path.strip_prefix("~/") {
        Some(rest) => format!("~/{}", shell_quote(rest)),
        None if path == "~" => "~".to_string(),
        None => shell_quote(path),
    }
}

/// Register the `ssh://` transport with libgit2. Safe to call repeatedly;
/// only the first call does anything. Must run before any remote operation on
/// an `ssh://` URL.
pub fn register() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        // libgit2 appends `://` to the scheme itself. Custom transports take
        // precedence over the built-in ones, so `file` replaces libgit2's own
        // local transport.
        for scheme in ["ssh", "file"] {
            // SAFETY: `register` must not race with other transport creation;
            // we call it exactly once per scheme, before any remote is used,
            // guarded by `Once`.
            let result = unsafe {
                git2::transport::register(scheme, |remote| {
                    Transport::smart(remote, false, ExecSubtransport::default())
                })
            };
            if let Err(e) = result {
                eprintln!("Warning: could not register the {scheme} transport: {e}");
            }
        }
    });
}

/// How to reach the repository behind a URL.
enum Target {
    Ssh(SshUrl),
    Local(String),
}

impl Target {
    fn parse(url: &str) -> Result<Target, String> {
        if is_ssh_url(url) {
            return SshUrl::parse(url).map(Target::Ssh);
        }
        file_url_path(url)
            .map(Target::Local)
            .ok_or_else(|| format!("Unsupported URL: {url}"))
    }

    /// Human-readable name of the other end, for error messages.
    fn describe(&self) -> String {
        match self {
            Target::Ssh(u) => u.destination(),
            Target::Local(p) => p.clone(),
        }
    }

    /// The process that speaks the git protocol for `service` on its
    /// stdin/stdout.
    fn command(&self, service: Service) -> Command {
        let (dashed, sub) = match service {
            Service::UploadPackLs | Service::UploadPack => ("git-upload-pack", "upload-pack"),
            Service::ReceivePackLs | Service::ReceivePack => ("git-receive-pack", "receive-pack"),
        };
        match self {
            Target::Ssh(url) => {
                // `~/x` is passed as the relative `x`: ssh runs the command in
                // the login user's home directory, and hosts with a restricted
                // git shell (GitHub-style `user/repo.git`) accept only that form.
                let path = url.path.strip_prefix("~/").unwrap_or(&url.path);
                let mut cmd = url.command();
                cmd.arg(format!("{dashed} {}", shell_quote(path)));
                cmd
            }
            Target::Local(path) => {
                let mut cmd = Command::new("git");
                cmd.arg(sub).arg(path);
                cmd
            }
        }
    }
}

/// Shared handle to the running `ssh` process.
type SharedChild = Arc<Mutex<Child>>;
/// Everything `ssh` printed to stderr so far; surfaced in error messages.
type StderrBuf = Arc<Mutex<Vec<u8>>>;

#[derive(Default)]
struct ExecSubtransport {
    current: Mutex<Option<SharedChild>>,
}

impl SmartSubtransport for ExecSubtransport {
    fn action(
        &self,
        url: &str,
        action: Service,
    ) -> Result<Box<dyn SmartSubtransportStream>, git2::Error> {
        let target = Target::parse(url).map_err(|e| git2::Error::from_str(&e))?;
        let mut command = target.command(action);
        let program = command.get_program().to_string_lossy().into_owned();
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| git2::Error::from_str(&format!("Failed to start '{program}': {e}")))?;

        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");

        let stderr_buf: StderrBuf = Arc::default();
        {
            let buf = stderr_buf.clone();
            std::thread::spawn(move || {
                let mut stderr = stderr;
                let mut chunk = [0u8; 1024];
                while let Ok(n) = stderr.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    buf.lock().unwrap().extend_from_slice(&chunk[..n]);
                }
            });
        }

        let child: SharedChild = Arc::new(Mutex::new(child));
        *self.current.lock().unwrap() = Some(child.clone());

        Ok(Box::new(ExecStream {
            is_ssh: matches!(target, Target::Ssh(_)),
            host: target.describe(),
            child,
            stdin: Some(stdin),
            stdout,
            stderr: stderr_buf,
        }))
    }

    fn close(&self) -> Result<(), git2::Error> {
        let Some(child) = self.current.lock().unwrap().take() else {
            return Ok(());
        };
        let mut child = child.lock().unwrap();
        // libgit2 has released the stream (and with it ssh's stdin) by now, so
        // the remote command sees EOF and ssh exits on its own; give it a moment
        // before pulling the plug.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return Ok(()),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Ok(());
                }
                Err(e) => return Err(git2::Error::from_str(&format!("ssh: {e}"))),
            }
        }
    }
}

/// The protocol stream: writes go to the child's stdin, reads come from its
/// stdout.
struct ExecStream {
    is_ssh: bool,
    host: String,
    child: SharedChild,
    stdin: Option<ChildStdin>,
    stdout: ChildStdout,
    stderr: StderrBuf,
}

impl ExecStream {
    /// Turn an early EOF into a useful error when `ssh` (or the remote command)
    /// failed, quoting what it printed to stderr.
    fn failure_message(&self) -> Option<String> {
        let status = self.child.lock().unwrap().try_wait().ok().flatten()?;
        if status.success() {
            return None;
        }
        // stderr is drained by a helper thread; give it a moment to catch up.
        std::thread::sleep(Duration::from_millis(50));
        let stderr = String::from_utf8_lossy(&self.stderr.lock().unwrap())
            .trim()
            .to_string();
        let detail = if stderr.is_empty() {
            format!("exit status {status}")
        } else {
            stderr
        };
        Some(if self.is_ssh && status.code() == Some(SSH_EXIT_FAILURE) {
            format!("Could not connect to {}: {detail}", self.host)
        } else if self.is_ssh {
            format!("Remote command on {} failed: {detail}", self.host)
        } else {
            format!("git failed for {}: {detail}", self.host)
        })
    }
}

impl Read for ExecStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.stdout.read(buf)?;
        if n == 0
            && let Some(msg) = self.failure_message()
        {
            return Err(io::Error::other(msg));
        }
        Ok(n)
    }
}

impl Write for ExecStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.stdin.as_mut() {
            Some(stdin) => stdin.write(buf),
            None => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "connection already closed",
            )),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.stdin.as_mut() {
            Some(stdin) => stdin.flush(),
            None => Ok(()),
        }
    }
}

/// Result of probing a remote machine for a Piki notes directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteCheck {
    /// Connected, and the directory is a Git repository ready to sync with.
    Ok,
    /// `ssh` could not connect or log in (unreachable host, no usable key).
    Unreachable(String),
    /// Connected fine, but there is no Git repository at the given path.
    NoRepository(String),
}

/// Connect to the machine named in `url` and check that its notes directory
/// is a Git repository. On success the remote repository is also configured to
/// accept pushes to its checked-out branch (`receive.denyCurrentBranch =
/// updateInstead`), updating its working tree when clean, refusing otherwise —
/// the piece of setup that makes pushing between two working copies possible.
pub fn check_remote_repository(url: &SshUrl) -> Result<RemoteCheck, String> {
    let dir = shell_quote_path(&url.path);
    let script = format!(
        "cd {dir} && git rev-parse --is-inside-work-tree >/dev/null && \
         git config receive.denyCurrentBranch updateInstead"
    );
    let output = url
        .command()
        .arg(script)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Failed to start '{}': {e}", ssh_program()))?;
    if output.status.success() {
        return Ok(RemoteCheck::Ok);
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let detail = if stderr.is_empty() {
        format!("exit status {}", output.status)
    } else {
        stderr
    };
    if output.status.code() == Some(SSH_EXIT_FAILURE) {
        Ok(RemoteCheck::Unreachable(detail))
    } else {
        Ok(RemoteCheck::NoRepository(detail))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_host_with_home_relative_path() {
        let u = SshUrl::parse("ssh://laptop/~/.piki").unwrap();
        assert_eq!(u.user, None);
        assert_eq!(u.host, "laptop");
        assert_eq!(u.port, None);
        assert_eq!(u.path, "~/.piki");
        assert_eq!(u.destination(), "laptop");
        assert_eq!(u.to_url(), "ssh://laptop/~/.piki");
    }

    #[test]
    fn parses_user_port_and_absolute_path() {
        let u = SshUrl::parse("ssh://rob@nas.local:2222/srv/notes").unwrap();
        assert_eq!(u.user.as_deref(), Some("rob"));
        assert_eq!(u.host, "nas.local");
        assert_eq!(u.port, Some(2222));
        assert_eq!(u.path, "/srv/notes");
        assert_eq!(u.destination(), "rob@nas.local");
        assert_eq!(u.to_url(), "ssh://rob@nas.local:2222/srv/notes");
    }

    #[test]
    fn rejects_bad_urls() {
        assert!(SshUrl::parse("laptop:.piki").is_err());
        assert!(SshUrl::parse("ssh:///nohost").is_err());
        assert!(SshUrl::parse("ssh://host:notaport/x").is_err());
    }

    #[test]
    fn host_shorthand_becomes_default_notes_url() {
        assert_eq!(
            url_for_host("laptop", None).unwrap(),
            "ssh://laptop/~/.piki"
        );
        assert_eq!(
            url_for_host("rob@laptop:2200", None).unwrap(),
            "ssh://rob@laptop:2200/~/.piki"
        );
        assert_eq!(
            url_for_host("laptop", Some("~/notes")).unwrap(),
            "ssh://laptop/~/notes"
        );
        assert_eq!(
            url_for_host("laptop", Some("notes/wiki")).unwrap(),
            "ssh://laptop/~/notes/wiki"
        );
        assert_eq!(
            url_for_host("laptop", Some("/srv/wiki")).unwrap(),
            "ssh://laptop/srv/wiki"
        );
        assert!(url_for_host("", None).is_err());
        assert!(url_for_host("not a host", None).is_err());
        assert!(url_for_host("host/path", None).is_err());
    }

    #[test]
    fn scp_style_urls_are_normalized() {
        assert_eq!(normalize_url("laptop:.piki"), "ssh://laptop/~/.piki");
        assert_eq!(normalize_url("laptop:~/.piki"), "ssh://laptop/~/.piki");
        assert_eq!(normalize_url("rob@laptop:/srv/x"), "ssh://rob@laptop/srv/x");
        assert_eq!(
            normalize_url("ssh://laptop/~/.piki"),
            "ssh://laptop/~/.piki"
        );
        assert_eq!(normalize_url("file:///tmp/x"), "file:///tmp/x");
        assert_eq!(normalize_url("/tmp/x"), "file:///tmp/x");
        assert_eq!(normalize_url("C:\\notes\\wiki"), "file:///C:/notes/wiki");
        assert_eq!(normalize_url("./relative"), "./relative");
        assert_eq!(file_url_path("file:///tmp/x").unwrap(), "/tmp/x");
        assert_eq!(file_url_path("file:///C:/x").unwrap(), "C:/x");
    }

    #[test]
    fn shell_quoting() {
        assert_eq!(shell_quote("plain"), "'plain'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote_path("~/.piki"), "~/'.piki'");
        assert_eq!(shell_quote_path("/srv/my notes"), "'/srv/my notes'");
        assert_eq!(shell_quote_path("~"), "~");
    }
}
