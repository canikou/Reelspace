//! Loopback HTTP file server for in-app playback on Linux. WebKitGTK's GStreamer
//! media pipeline cannot stream from custom URI schemes (asset://), but plays
//! from http:// with Range support. URL: `http://127.0.0.1:PORT/TOKEN/<abs path>`.
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

type Allow = Arc<dyn Fn(&Path) -> bool + Send + Sync>;

/// Starts the server on a random loopback port; returns the base URL (with token).
pub fn start(allowed: Allow) -> std::io::Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let token = format!("{:032x}", rand::random::<u128>());
    let base = format!("http://127.0.0.1:{}/{token}", listener.local_addr()?.port());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (token, allowed) = (token.clone(), allowed.clone());
            std::thread::spawn(move || {
                let _ = handle(stream, &token, &*allowed);
            });
        }
    });
    Ok(base)
}

/// Parses an HTTP Range header against a file length.
/// `None` = serve everything (absent or unsupported), `Some(Err)` = unsatisfiable.
fn parse_range(header: Option<&str>, len: u64) -> Option<Result<(u64, u64), ()>> {
    let spec = header?.trim().strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None; // shortcut: multi-range served as a full 200, upgrade if a player needs it
    }
    let (a, b) = spec.split_once('-')?;
    let range = match (a.trim(), b.trim()) {
        ("", n) => n
            .parse::<u64>()
            .ok()
            .filter(|n| *n > 0)
            .map(|n| (len.saturating_sub(n), len.wrapping_sub(1))),
        (s, "") => s.parse::<u64>().ok().map(|s| (s, len.wrapping_sub(1))),
        (s, e) => s
            .parse::<u64>()
            .ok()
            .zip(e.parse::<u64>().ok())
            .map(|(s, e)| (s, e.min(len.wrapping_sub(1)))),
    };
    Some(match range {
        Some((s, e)) if len > 0 && s <= e && s < len => Ok((s, e)),
        _ => Err(()),
    })
}

fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("mp4" | "m4v") => "video/mp4",
        Some("mkv") => "video/x-matroska",
        Some("webm") => "video/webm",
        Some("mov") => "video/quicktime",
        Some("m4a") => "audio/mp4",
        Some("wav") => "audio/wav",
        Some("ogg" | "opus") => "audio/ogg",
        Some("mp3") => "audio/mpeg",
        _ => "application/octet-stream",
    }
}

fn percent_decode(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    Some(out)
}

fn status(out: &mut impl Write, code: &str) -> std::io::Result<()> {
    write!(
        out,
        "HTTP/1.1 {code}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
}

fn handle(stream: TcpStream, token: &str, allowed: &dyn Fn(&Path) -> bool) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut out = stream.try_clone()?;
    let mut reader = BufReader::new(stream.take(16 * 1024));
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let request = line.clone();
    let mut parts = request.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let mut range = None;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 || line.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case("range") {
                range = Some(v.trim().to_string());
            }
        }
    }
    if method != "GET" && method != "HEAD" {
        return status(&mut out, "405 Method Not Allowed");
    }
    let rest = target.split(['?', '#']).next().unwrap_or("");
    let Some(rest) = rest.strip_prefix('/').and_then(|r| r.strip_prefix(token)) else {
        return status(&mut out, "403 Forbidden");
    };
    let Some(decoded) =
        percent_decode(rest).filter(|d| d.first() == Some(&b'/') && !d.contains(&0))
    else {
        return status(&mut out, "400 Bad Request");
    };
    let path = PathBuf::from(std::ffi::OsString::from_vec(decoded));
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return status(&mut out, "403 Forbidden");
    }
    let Ok(path) = path.canonicalize() else {
        return status(&mut out, "404 Not Found");
    };
    if !allowed(&path) {
        return status(&mut out, "403 Forbidden");
    }
    let Ok(mut file) = File::open(&path)
        .and_then(|f| f.metadata().map(|m| (f, m)))
        .and_then(|(f, m)| {
            m.is_file()
                .then_some(f)
                .ok_or_else(|| std::io::ErrorKind::NotFound.into())
        })
    else {
        return status(&mut out, "404 Not Found");
    };
    let len = file.metadata()?.len();
    let ctype = content_type(&path);
    let (code, start, end, extra) = match parse_range(range.as_deref(), len) {
        None => ("200 OK", 0, len.wrapping_sub(1), String::new()),
        Some(Ok((s, e))) => ("206 Partial Content", s, e, format!("Content-Range: bytes {s}-{e}/{len}\r\n")),
        Some(Err(())) => {
            return write!(
                out,
                "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{len}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
        }
    };
    let body_len = if len == 0 { 0 } else { end - start + 1 };
    write!(
        out,
        "HTTP/1.1 {code}\r\nContent-Type: {ctype}\r\nAccept-Ranges: bytes\r\n{extra}Content-Length: {body_len}\r\nConnection: close\r\n\r\n"
    )?;
    if method == "GET" && body_len > 0 {
        file.seek(SeekFrom::Start(start))?;
        std::io::copy(&mut file.take(body_len), &mut out)?; // client disconnects surface as Err and are dropped by the caller
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_parsing() {
        assert_eq!(parse_range(None, 100), None);
        assert_eq!(parse_range(Some("bytes=0-9"), 100), Some(Ok((0, 9))));
        assert_eq!(parse_range(Some("bytes=90-"), 100), Some(Ok((90, 99))));
        assert_eq!(parse_range(Some("bytes=-10"), 100), Some(Ok((90, 99))));
        assert_eq!(parse_range(Some("bytes=-500"), 100), Some(Ok((0, 99))));
        assert_eq!(parse_range(Some("bytes=50-500"), 100), Some(Ok((50, 99))));
        assert_eq!(parse_range(Some("bytes=100-"), 100), Some(Err(())));
        assert_eq!(parse_range(Some("bytes=9-3"), 100), Some(Err(())));
        assert_eq!(parse_range(Some("bytes=0-"), 0), Some(Err(())));
        assert_eq!(parse_range(Some("bytes=0-1,5-6"), 100), None);
    }

    fn get(base: &str, tail: &str, range: Option<&str>) -> (String, Vec<u8>) {
        let addr = base
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap()
            .to_string();
        let token = base.rsplit('/').next().unwrap();
        let tail = tail.replace("{token}", token);
        let mut s = TcpStream::connect(addr).unwrap();
        let r = range.map(|r| format!("Range: {r}\r\n")).unwrap_or_default();
        write!(s, "GET {tail} HTTP/1.1\r\nHost: x\r\n{r}\r\n").unwrap();
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).unwrap();
        let split = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        (
            String::from_utf8_lossy(&buf[..split]).into_owned(),
            buf[split..].to_vec(),
        )
    }

    #[test]
    fn serves_ranges_and_rejects_bad_requests() {
        let dir = tempfile::tempdir().unwrap();
        let allowed_dir = dir.path().canonicalize().unwrap();
        let file = allowed_dir.join("a b.mp4");
        let data: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        std::fs::write(&file, &data).unwrap();
        std::fs::create_dir(allowed_dir.join("sub")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("x.mp4"), b"secret").unwrap();
        let root = allowed_dir.clone();
        let base = start(Arc::new(move |p: &Path| p.starts_with(&root))).unwrap();
        let url = format!("/{{token}}{}", file.to_string_lossy().replace(' ', "%20"));

        let (head, body) = get(&base, &url, None);
        assert!(
            head.starts_with("HTTP/1.1 200")
                && head.contains("video/mp4")
                && head.contains("Accept-Ranges: bytes")
        );
        assert_eq!(body, data);

        let (head, body) = get(&base, &url, Some("bytes=10-19"));
        assert!(
            head.starts_with("HTTP/1.1 206") && head.contains("Content-Range: bytes 10-19/1000")
        );
        assert_eq!(body, &data[10..20]);
        assert_eq!(get(&base, &url, Some("bytes=-5")).1, &data[995..]);
        assert!(get(&base, &url, Some("bytes=1000-"))
            .0
            .starts_with("HTTP/1.1 416"));

        assert!(get(&base, &url.replace("{token}", "bad"), None)
            .0
            .starts_with("HTTP/1.1 403"));
        let out = outside.path().join("x.mp4");
        assert!(get(&base, &format!("/{{token}}{}", out.display()), None)
            .0
            .starts_with("HTTP/1.1 403"));
        let dotdot = format!(
            "/{{token}}{}/sub/../../{}/x.mp4",
            allowed_dir.display(),
            outside.path().file_name().unwrap().to_string_lossy()
        );
        assert!(get(&base, &dotdot, None).0.starts_with("HTTP/1.1 403"));
        let encoded = format!("/{{token}}{}/sub/%2e%2e/a%20b.mp4", allowed_dir.display());
        assert!(get(&base, &encoded, None).0.starts_with("HTTP/1.1 403"));
        let dir_url = format!("/{{token}}{}/sub", allowed_dir.display());
        assert!(get(&base, &dir_url, None).0.starts_with("HTTP/1.1 404"));
    }
}
