use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct VolumePreference {
    pub target: u8,
}

#[derive(Default, Clone, PartialEq, Eq)]
pub struct Config {
    pub load_failed: bool,
    pub known_apps: BTreeMap<String, String>,
    pub hidden_apps: BTreeMap<String, crate::model::HiddenApp>,
    pub routes: BTreeMap<String, String>,
    pub explicit_routes: BTreeSet<String>,
    pub managed_default: bool,
    pub default_device: Option<String>,
    pub volumes: BTreeMap<String, VolumePreference>,
    pub device_mutes: BTreeMap<String, bool>,
    pub app_mutes: BTreeMap<String, bool>,
}

pub fn directory() -> PathBuf {
    std::env::current_exe().ok().and_then(|path| path.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn legacy_directory(name: &str) -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir).join(name)
}

fn ini_escape(s: &str) -> String {
    s.chars().map(|c| match c {
        '%' => "%25".into(), '=' => "%3D".into(), '\n' => "%0A".into(),
        '\r' => "%0D".into(), ' ' => "%20".into(), '\t' => "%09".into(),
        '[' => "%5B".into(), ']' => "%5D".into(), ';' => "%3B".into(), '#' => "%23".into(),
        c => c.to_string(),
    }).collect()
}
fn ini_unescape(s: &str) -> Option<String> {
    let bytes = s.as_bytes(); let mut result = Vec::new(); let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let value = std::str::from_utf8(bytes.get(i+1..i+3)?).ok()?;
            result.push(u8::from_str_radix(value,16).ok()?); i += 3;
        } else { result.push(bytes[i]); i += 1; }
    }
    String::from_utf8(result).ok()
}

fn decode(s: &str) -> Option<String> {
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        return None;
    }
    let bytes: Option<Vec<_>> = (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect();
    String::from_utf8(bytes?).ok()
}

impl Config {
    pub fn load() -> Self {
        let ini = directory().join("AudioRouter.ini");
        if ini.is_file() {
            return std::fs::read_to_string(&ini).ok().filter(|s| s.len() <= 1_048_576)
                .and_then(|text| Self::parse_ini(&text)).unwrap_or_else(|| {
                    log("Invalid AudioRouter.ini; original file will not be overwritten.");
                    Self { load_failed: true, ..Self::default() }
                });
        }
        let current = legacy_directory("AudioRouter").join("preferences.txt");
        let legacy = legacy_directory("WinSpeaker").join("preferences.txt");
        let file = if current.is_file() { current } else { legacy };
        if !file.is_file() { return Self::default(); }
        let result = (|| -> Option<Self> {
            if std::fs::metadata(&file).ok()?.len() > 1_048_576 {
                return None;
            }
            let text = std::fs::read_to_string(&file).ok()?;
            let mut lines = text.lines();
            let version = lines.next()?;
            if !matches!(version, "AudioRouter-v2" | "AudioRouter-v1" | "WinSpeaker-v1") {
                return None;
            }
            let mut config = Self::default();
            for line in lines {
                let p: Vec<_> = line.split('\t').collect();
                match p.as_slice() {
                    ["managed", "1"] => { config.managed_default = true; }
                    ["explicit", key] => { config.explicit_routes.insert(decode(key)?); }
                    ["route", key, value] => {
                        config.routes.insert(decode(key)?, decode(value)?);
                    }
                    ["volume", key, target] if version == "AudioRouter-v2" => {
                        let target = target.parse::<u8>().ok()?;
                        if target > 100 { return None; }
                        config.volumes.insert(decode(key)?,VolumePreference { target });
                    }
                    ["volume", _, _, _] if version != "AudioRouter-v2" => {}
                    ["collapsed", _] => {}
                    _ => return None,
                }
            }
            Some(config)
        })();
        let config = result.unwrap_or_else(|| {
            log("Legacy preferences could not be read; using defaults."); Self::default()
        });
        if let Err(error) = config.save() { log(&format!("INI migration failed: {error}")); }
        config
    }

    fn parse_ini(text: &str) -> Option<Self> {
        let mut config = Self::default();
        let mut probe_section = "";
        let mut version = "5";
        for line in text.trim_start_matches('\u{feff}').lines() {
            let line = line.trim();
            if let Some(name) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                probe_section = name;
            } else if probe_section == "General" {
                if let Some((key,value)) = line.split_once('=') {
                    if key.trim() == "Version" { version = value.trim(); }
                }
            }
        }
        if !matches!(version, "3" | "4" | "5") { return None; }
        let mut section = "";
        for line in text.trim_start_matches('\u{feff}').lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') { continue; }
            if let Some(name) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                section = name; continue;
            }
            let (key,value) = line.split_once('=')?;
            let key = ini_unescape(key.trim())?;
            let value = ini_unescape(value.trim())?;
            match section {
                "General" => match key.as_str() {
                    "Version" if !matches!(value.as_str(), "3" | "4" | "5") => return None,
                    "DefaultDevice" => config.default_device = if value.is_empty() { None } else { Some(value) },
                    "ManagedDefault" => config.managed_default = value == "1",
                    _ => {}
                },
                "KnownApps" => { config.known_apps.insert(key,value); }
                "Routes" => {
                    if version != "3" { config.explicit_routes.insert(key.clone()); }
                    config.routes.insert(key,value);
                }
                "ImportedRoutes" if version == "4" => { config.routes.insert(key,value); }
                "ExplicitRoutes" if value == "1" => { config.explicit_routes.insert(key); }
                "Volumes" => { let target = value.parse::<u8>().ok()?; if target > 100 { return None; } config.volumes.insert(key,VolumePreference { target }); }
                "DeviceMute" | "AppMute" => {
                    let muted = match value.as_str() { "1" => true, "0" => false, _ => return None };
                    if section == "DeviceMute" { config.device_mutes.insert(key,muted); }
                    else { config.app_mutes.insert(key,muted); }
                }
                "HiddenApps" => {
                    let (time,name) = value.split_once('|')?;
                    let hidden_at = time.parse::<u64>().ok()?;
                    config.hidden_apps.insert(key.clone(),crate::model::HiddenApp {
                        key, name: name.to_string(), hidden_at,
                    });
                }
                _ => {}
            }
        }
        config.routes.retain(|key,_| config.explicit_routes.contains(key));
        config.explicit_routes.retain(|key| config.routes.contains_key(key));
        config.managed_default = config.default_device.is_some();
        Some(config)
    }

    pub fn compact_file() -> Result<(),String> {
        let path = directory().join("AudioRouter.ini");
        if !path.is_file() { return Ok(()); }
        let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let config = Self::parse_ini(&text).ok_or("Invalid INI; original file preserved")?;
        if !text.lines().any(|line| line.trim().starts_with("Version=")) { return Ok(()); }
        let backup = directory().join("AudioRouter.ini.bak");
        std::fs::copy(&path,&backup).map_err(|e| e.to_string())?;
        config.save()
    }

    pub fn save(&self) -> Result<(), String> {
        if self.load_failed { return Err("Original INI could not be read; refusing to overwrite it".into()); }
        let dir = directory();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut text = String::from("[General]\n");
        if let Some(device) = &self.default_device { text.push_str(&format!("DefaultDevice={}\n",ini_escape(device))); }
        if !self.known_apps.is_empty() {
            text.push_str("\n[KnownApps]\n");
            for (key,name) in &self.known_apps {
                text.push_str(&format!("{}={}\n",ini_escape(key),ini_escape(name)));
            }
        }
        let routes: Vec<_> = self.routes.iter().filter(|(key,_)| self.explicit_routes.contains(*key)).collect();
        if !routes.is_empty() {
            text.push_str("\n[Routes]\n");
            for (key,value) in routes { text.push_str(&format!("{}={}\n",ini_escape(key),ini_escape(value))); }
        }
        let volumes: Vec<_> = self.volumes.iter().filter(|(_,value)| value.target != 100).collect();
        if !volumes.is_empty() {
            text.push_str("\n[Volumes]\n");
            for (key,value) in volumes { text.push_str(&format!("{}={}\n",ini_escape(key),value.target)); }
        }
        if !self.hidden_apps.is_empty() {
            text.push_str("\n[HiddenApps]\n");
            let mut hidden: Vec<_> = self.hidden_apps.values().collect();
            hidden.sort_by_key(|app| app.hidden_at);
            for app in hidden {
                text.push_str(&format!("{}={}\n",ini_escape(&app.key),ini_escape(&format!("{}|{}",app.hidden_at,app.name))));
            }
        }
        for (section,values) in [("DeviceMute",&self.device_mutes),("AppMute",&self.app_mutes)] {
            if !values.is_empty() {
                text.push_str(&format!("\n[{section}]\n"));
                for (key,muted) in values { text.push_str(&format!("{}={}\n",ini_escape(key),u8::from(*muted))); }
            }
        }
        let mut expected = self.clone();
        expected.volumes.retain(|_,value| value.target != 100);
        expected.routes.retain(|key,_| expected.explicit_routes.contains(key));
        expected.explicit_routes.retain(|key| expected.routes.contains_key(key));
        expected.managed_default = expected.default_device.is_some();
        if Self::parse_ini(&text) != Some(expected) {
            return Err("Configuration round-trip mismatch; original INI preserved".into());
        }
        let temporary = dir.join("AudioRouter.ini.tmp");
        let mut file = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
        file.write_all(text.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        std::fs::rename(temporary, dir.join("AudioRouter.ini")).map_err(|e| e.to_string())
    }
}

pub fn log(message: &str) {
    let dir = directory();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("audiorouter.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 524_288) {
        let _ = std::fs::rename(&path, dir.join("audiorouter.previous.log"));
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(file, "[{time}] {message}");
    }
}
