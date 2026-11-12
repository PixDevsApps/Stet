//! Check for Updates (1.5): one GET of GitHub's answer for Stet's latest release, made by
//! `curl` only when the user runs the command. It carries nothing about the user: the User-Agent
//! `Stet`, no cookies, no Referer, no proxy, and no `~/.curlrc`. Blocking: run it on a worker.

use std::path::Path;
use std::process::{Command, Stdio};
use stet_domain::updates::{MAX_ANSWER, UpdateError, Version, release_version};

/// Seconds to connect, and for the whole request.
const TIMEOUT: &str = "20";

/// The version of the latest release `url` names. `url` is HTTPS, or plain HTTP to this
/// computer for the tests.
pub fn latest(url: &str) -> Result<Version, UpdateError> {
    latest_with(Path::new("curl"), url)
}

/// [`latest`] with the `curl` at `curl`.
pub fn latest_with(curl: &Path, url: &str) -> Result<Version, UpdateError> {
    let protocol = if url.starts_with("https://") {
        "=https"
    } else if url.starts_with("http://127.0.0.1:") || url.starts_with("http://[::1]:") {
        "=http"
    } else {
        return Err(UpdateError::Unreadable);
    };
    let output = Command::new(curl)
        // `-q` first: no `.curlrc`.
        .args(["-q", "--silent", "--location", "--max-redirs", "3"])
        .args(["--proto", protocol, "--proto-redir", protocol])
        .args(["--connect-timeout", TIMEOUT, "--max-time", TIMEOUT])
        .args(["--max-filesize", &MAX_ANSWER.to_string()])
        .args(["--noproxy", "*", "--user-agent", "Stet"])
        .args(["--header", "Accept: application/vnd.github+json"])
        .args(["--write-out", "\n%{http_code}", url])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| {
            tracing::info!(%error, "could not run curl for the update check");
            UpdateError::NoCurl
        })?;
    if !output.status.success() {
        let code = output.status.code();
        tracing::info!(?code, "curl got no answer for the update check");
        return Err(match code {
            Some(28) => UpdateError::Timeout,
            Some(51 | 58 | 60 | 77 | 83 | 90 | 91) => UpdateError::Certificate,
            Some(63) => UpdateError::Unreadable,
            _ => UpdateError::Offline,
        });
    }
    let stdout = output.stdout;
    let split = stdout
        .iter()
        .rposition(|byte| *byte == b'\n')
        .ok_or(UpdateError::Unreadable)?;
    let (answer, status) = (&stdout[..split], &stdout[split + 1..]);
    match status {
        b"200" => release_version(answer),
        b"404" => Err(UpdateError::NoRelease),
        b"403" | b"429" => Err(UpdateError::Busy),
        _ => {
            tracing::info!(
                status = %String::from_utf8_lossy(status),
                "the update check got no release"
            );
            Err(UpdateError::Unreadable)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread::JoinHandle;

    /// A server on 127.0.0.1 that answers one request with `status` and `body`; joining it gives
    /// the request it read.
    fn server(status: &'static str, body: String) -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/repos/PixDevsApps/Stet/releases/latest",
            listener.local_addr().unwrap()
        );
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }
            let answer = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(answer.as_bytes());
            String::from_utf8_lossy(&request).into_owned()
        });
        (url, thread)
    }

    #[test]
    fn reads_the_latest_tag_and_sends_nothing_about_the_user() {
        let body = r#"{"tag_name": "v1.6.0", "name": "Stet 1.6.0", "draft": false,
            "assets": [{"name": "stet-x86_64.pkg.tar.zst"}]}"#;
        let (url, thread) = server("200 OK", body.to_owned());
        assert_eq!(
            latest(&url),
            Ok(Version {
                major: 1,
                minor: 6,
                patch: 0
            })
        );
        let request = thread.join().unwrap().to_ascii_lowercase();
        assert!(
            request.starts_with("get /repos/pixdevsapps/stet/releases/latest http/1.1\r\n"),
            "{request}"
        );
        assert!(request.contains("\r\nuser-agent: stet\r\n"), "{request}");
        assert!(
            request.contains("\r\naccept: application/vnd.github+json\r\n"),
            "{request}"
        );
        for header in ["cookie", "referer", "authorization"] {
            assert!(!request.contains(&format!("\r\n{header}:")), "{request}");
        }
    }

    #[test]
    fn refusals_and_answers_without_a_version() {
        for (status, body, error) in [
            ("404 Not Found", "{}", UpdateError::NoRelease),
            ("403 Forbidden", "{}", UpdateError::Busy),
            ("429 Too Many Requests", "{}", UpdateError::Busy),
            ("500 Internal Server Error", "{}", UpdateError::Unreadable),
            (
                "200 OK",
                r#"{"tag_name": "nightly"}"#,
                UpdateError::Unreadable,
            ),
            ("200 OK", "not json", UpdateError::Unreadable),
        ] {
            let (url, thread) = server(status, body.to_owned());
            assert_eq!(latest(&url), Err(error), "{status} {body}");
            thread.join().unwrap();
        }
        let (url, thread) = server("200 OK", " ".repeat(MAX_ANSWER + 1));
        assert_eq!(latest(&url), Err(UpdateError::Unreadable));
        let _ = thread.join();
    }

    #[test]
    fn nothing_listening() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        drop(listener);
        assert_eq!(latest(&url), Err(UpdateError::Offline));
    }

    #[test]
    fn only_https_or_this_computer() {
        for url in [
            "http://api.github.com/repos/PixDevsApps/Stet/releases/latest",
            "http://127.0.0.1.example.com/",
            "ftp://127.0.0.1/",
            "file:///etc/passwd",
            "-K/etc/passwd",
            "",
        ] {
            assert_eq!(latest(url), Err(UpdateError::Unreadable), "{url}");
        }
    }

    #[test]
    fn without_curl() {
        assert_eq!(
            latest_with(
                Path::new("/nonexistent/curl"),
                "https://api.github.com/repos/PixDevsApps/Stet/releases/latest"
            ),
            Err(UpdateError::NoCurl)
        );
    }

    /// The real answer: `cargo test -p stet-infrastructure updates::tests::github -- --ignored`.
    #[test]
    #[ignore = "reaches api.github.com"]
    fn github() {
        let latest = latest(&stet_domain::updates::latest_release_url()).unwrap();
        println!("the latest release is {latest}");
    }
}
