//! Game detection (Steam reaper, process names, focused window), session tracking, duplicate-capture guard.
use super::config::SessionConfig;
use std::path::{Path, PathBuf};

// ---------- VDF / ACF ----------

#[derive(Debug, Clone, PartialEq)]
pub enum Vdf {
    Str(String),
    Map(Vec<(String, Vdf)>),
}

impl Vdf {
    pub fn get(&self, key: &str) -> Option<&Vdf> {
        match self {
            Vdf::Map(m) => m.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            Vdf::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// Minimal Valve KeyValues parser: quoted strings, `{}` blocks, `//` comments. Returns the root map.
pub fn parse_vdf(text: &str) -> Option<Vdf> {
    parse_tokens(text)
}

fn parse_tokens(text: &str) -> Option<Vdf> {
    #[derive(PartialEq, Clone)]
    enum T {
        S(String),
        Open,
        Close,
    }
    let mut toks = vec![];
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' => {
                let mut s = String::new();
                loop {
                    match it.next()? {
                        '"' => break,
                        '\\' => match it.next()? {
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            o => s.push(o),
                        },
                        c => s.push(c),
                    }
                }
                toks.push(T::S(s));
            }
            '{' => toks.push(T::Open),
            '}' => toks.push(T::Close),
            '/' if it.peek() == Some(&'/') => {
                for c in it.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    fn block(toks: &[T], pos: &mut usize, top: bool) -> Option<Vdf> {
        let mut out = vec![];
        while *pos < toks.len() {
            match &toks[*pos] {
                T::Close => {
                    *pos += 1;
                    return (!top).then_some(Vdf::Map(out));
                }
                T::S(k) => {
                    *pos += 1;
                    match toks.get(*pos)? {
                        T::S(v) => {
                            out.push((k.clone(), Vdf::Str(v.clone())));
                            *pos += 1;
                        }
                        T::Open => {
                            *pos += 1;
                            out.push((k.clone(), block(toks, pos, false)?));
                        }
                        T::Close => return None,
                    }
                }
                T::Open => return None,
            }
        }
        top.then_some(Vdf::Map(out))
    }
    let mut pos = 0;
    block(&toks, &mut pos, true)
}

pub fn library_paths(vdf_text: &str) -> Vec<PathBuf> {
    let Some(root) = parse_vdf(vdf_text) else { return vec![] };
    let Some(Vdf::Map(libs)) = root.get("libraryfolders") else { return vec![] };
    libs.iter().filter_map(|(_, v)| v.get("path")?.str().map(PathBuf::from)).collect()
}

pub fn manifest_name(acf_text: &str) -> Option<String> {
    parse_vdf(acf_text)?.get("AppState")?.get("name")?.str().map(str::to_string)
}

fn steam_roots() -> Vec<PathBuf> {
    let h = super::config::home();
    vec![h.join(".local/share/Steam"), h.join(".steam/steam"), h.join(".var/app/com.valvesoftware.Steam/.local/share/Steam")]
}

/// Name of a Steam app from any library's appmanifest.
pub fn steam_app_name(appid: u32) -> Option<String> {
    for root in steam_roots() {
        let mut libs = vec![root.clone()];
        if let Ok(t) = std::fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) {
            libs.extend(library_paths(&t));
        }
        for l in libs {
            if let Ok(t) = std::fs::read_to_string(l.join(format!("steamapps/appmanifest_{appid}.acf"))) {
                if let Some(n) = manifest_name(&t) {
                    return Some(n);
                }
            }
        }
    }
    None
}

// ---------- /proc scanning ----------

pub type Proc = (u32, Vec<String>);

pub fn scan_procs(proc_root: &Path) -> Vec<Proc> {
    let Ok(rd) = std::fs::read_dir(proc_root) else { return vec![] };
    rd.filter_map(|e| {
        let e = e.ok()?;
        let pid: u32 = e.file_name().to_str()?.parse().ok()?;
        let raw = std::fs::read(e.path().join("cmdline")).ok()?;
        let args: Vec<String> = raw.split(|b| *b == 0).filter(|s| !s.is_empty()).map(|s| String::from_utf8_lossy(s).into_owned()).collect();
        (!args.is_empty()).then_some((pid, args))
    })
    .collect()
}

fn base(arg: &str) -> &str {
    arg.rsplit('/').next().unwrap_or(arg)
}

/// `…/reaper SteamLaunch AppId=123 -- …` -> 123
pub fn parse_reaper(args: &[String]) -> Option<u32> {
    if base(args.first()?) != "reaper" || !args.iter().any(|a| a == "SteamLaunch") {
        return None;
    }
    args.iter().find_map(|a| a.strip_prefix("AppId=")?.parse().ok())
}

/// Another gsr that would capture with a replay buffer / recording. `own_sock` excludes our own instance.
pub fn is_duplicate_capture(args: &[String], own_sock: &str) -> bool {
    base(args.first().map(String::as_str).unwrap_or("")) == "gpu-screen-recorder"
        && args.iter().any(|a| a == "-r" || a == "-replay-storage")
        && !args.iter().any(|a| a == own_sock)
}

// ---------- detection ----------

#[derive(Debug, Clone, PartialEq)]
pub struct Detected {
    pub key: String,
    pub name: String,
    pub appid: Option<u32>,
}

pub fn detect(procs: &[Proc], cfg: &SessionConfig, focused_app_id: Option<&str>, name_lookup: &dyn Fn(u32) -> Option<String>) -> Vec<Detected> {
    let mut out: Vec<Detected> = vec![];
    let mut steam: Vec<u32> = procs.iter().filter_map(|(_, a)| parse_reaper(a)).filter(|id| !cfg.deny_appids.contains(id)).collect();
    steam.sort_unstable();
    steam.dedup();
    for id in steam {
        out.push(Detected { key: format!("steam:{id}"), name: name_lookup(id).unwrap_or_else(|| format!("Steam App {id}")), appid: Some(id) });
    }
    for want in &cfg.game_processes {
        if procs.iter().any(|(_, a)| base(&a[0]) == want) && !out.iter().any(|d| d.key == format!("proc:{want}")) {
            out.push(Detected { key: format!("proc:{want}"), name: want.clone(), appid: None });
        }
    }
    if let Some(app) = focused_app_id.filter(|a| cfg.focus_app_ids.iter().any(|w| w == a)) {
        out.push(Detected { key: format!("focus:{app}"), name: app.to_string(), appid: None });
    }
    out
}

// ---------- session tracking ----------

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub primary: Detected,
    pub games: Vec<String>,
    pub started: std::time::Instant,
}

#[derive(Debug, PartialEq)]
pub enum SessionEvent {
    Started(Detected),
    GameAdded(String),
    Ended(Session),
}

/// Pure-ish state machine: feed it each poll's detections. Ends after `grace` consecutive empty polls.
pub struct Tracker {
    pub current: Option<Session>,
    empty_polls: u32,
    grace: u32,
}

impl Tracker {
    pub fn new(grace: u32) -> Self {
        Self { current: None, empty_polls: 0, grace }
    }

    pub fn update(&mut self, det: &[Detected], now: std::time::Instant) -> Vec<SessionEvent> {
        let mut ev = vec![];
        match (&mut self.current, det.first()) {
            (None, Some(d)) => {
                self.empty_polls = 0;
                self.current = Some(Session { primary: d.clone(), games: vec![d.name.clone()], started: now });
                ev.push(SessionEvent::Started(d.clone()));
            }
            (Some(s), Some(_)) => {
                self.empty_polls = 0;
                for d in det {
                    if !s.games.contains(&d.name) {
                        s.games.push(d.name.clone());
                        ev.push(SessionEvent::GameAdded(d.name.clone()));
                    }
                }
            }
            (Some(_), None) => {
                self.empty_polls += 1;
                if self.empty_polls >= self.grace {
                    ev.push(SessionEvent::Ended(self.current.take().unwrap()));
                }
            }
            (None, None) => {}
        }
        ev
    }
}

/// Session shorter than the minimum is discarded.
pub fn keep_session(duration_sec: u64, min_session_sec: u64) -> bool {
    duration_sec >= min_session_sec
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn a(s: &str) -> Vec<String> {
        s.split(' ').map(String::from).collect()
    }

    const ACF: &str = "\"AppState\"\n{\n\t\"appid\"\t\t\"1493710\"\n\t\"name\"\t\t\"Hades II: \\\"Test\\\"\"\n\t\"InstalledDepots\"\n\t{\n\t\t\"1\"\n\t\t{\n\t\t\t\"size\"\t\"5\"\n\t\t}\n\t}\n}\n";
    const LIBS: &str = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"/home/u/.local/share/Steam\"\n\t\t\"apps\"\n\t\t{\n\t\t\t\"1\"\t\"2\"\n\t\t}\n\t}\n\t// c\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"/mnt/data/Games\"\n\t}\n}\n";

    #[test]
    fn vdf_fixtures() {
        assert_eq!(manifest_name(ACF).as_deref(), Some("Hades II: \"Test\""));
        assert_eq!(library_paths(LIBS), vec![PathBuf::from("/home/u/.local/share/Steam"), PathBuf::from("/mnt/data/Games")]);
        assert!(parse_vdf("\"a\" {").is_none());
        assert!(manifest_name("garbage").is_none());
    }

    #[test]
    fn reaper_parsing() {
        assert_eq!(parse_reaper(&a("/home/u/.local/share/Steam/ubuntu12_32/reaper SteamLaunch AppId=2139460 -- /x/proton waitforexitandrun /g.exe")), Some(2139460));
        assert_eq!(parse_reaper(&a("reaper SteamLaunch AppId=abc")), None);
        assert_eq!(parse_reaper(&a("reaper other AppId=5")), None);
        assert_eq!(parse_reaper(&a("steam SteamLaunch AppId=5")), None);
    }

    #[test]
    fn duplicate_matcher() {
        let sock = "/run/u/reelspace-gsr.sock";
        assert!(is_duplicate_capture(&a("gpu-screen-recorder -w DP-1 -r 300 -o /x"), sock));
        assert!(is_duplicate_capture(&a("/usr/bin/gpu-screen-recorder -w DP-1 -replay-storage ram"), sock));
        assert!(!is_duplicate_capture(&a("gpu-screen-recorder -w DP-1 -o /x.mkv"), sock));
        assert!(!is_duplicate_capture(&a("gpu-screen-recorder -w DP-1 -r 300 -ipc /run/u/reelspace-gsr.sock"), sock));
        assert!(!is_duplicate_capture(&a("gsr-cli -ipc x -r"), sock));
    }

    #[test]
    fn deny_list_and_detection() {
        let procs = vec![
            (1, a("reaper SteamLaunch AppId=431960 --")),
            (2, a("reaper SteamLaunch AppId=228980 --")),
            (3, a("reaper SteamLaunch AppId=1493710 --")),
            (4, a("/opt/lutris/mygame.bin -x")),
        ];
        let cfg = SessionConfig { game_processes: vec!["mygame.bin".into()], focus_app_ids: vec!["org.x.Game".into()], ..Default::default() };
        let d = detect(&procs, &cfg, Some("org.x.Game"), &|id| (id == 1493710).then(|| "Hades II".to_string()));
        let keys: Vec<_> = d.iter().map(|d| d.key.as_str()).collect();
        assert_eq!(keys, vec!["steam:1493710", "proc:mygame.bin", "focus:org.x.Game"]);
        assert_eq!(d[0].name, "Hades II");
        assert!(detect(&procs, &cfg, Some("other"), &|_| None).iter().all(|d| d.key != "focus:other"));
    }

    #[test]
    fn tracker_sessions() {
        let g = |n: &str| Detected { key: n.into(), name: n.into(), appid: None };
        let t0 = Instant::now();
        let mut t = Tracker::new(3);
        assert_eq!(t.update(&[g("A")], t0), vec![SessionEvent::Started(g("A"))]);
        assert_eq!(t.update(&[g("A"), g("B")], t0), vec![SessionEvent::GameAdded("B".into())]);
        assert!(t.update(&[], t0).is_empty());
        assert!(t.update(&[g("A")], t0).is_empty()); // blip resets grace
        assert!(t.update(&[], t0).is_empty());
        assert!(t.update(&[], t0).is_empty());
        let ev = t.update(&[], t0 + Duration::from_secs(90));
        assert!(matches!(&ev[0], SessionEvent::Ended(s) if s.games == vec!["A", "B"]));
        assert!(t.current.is_none());
    }

    #[test]
    fn min_session() {
        assert!(!keep_session(59, 60));
        assert!(keep_session(60, 60));
    }
}
