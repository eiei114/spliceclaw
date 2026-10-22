use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rusqlite::{Connection, OpenFlags, params};
use zeroize::Zeroizing;

use crate::error::AppError;

pub(crate) struct Locations {
    pub firefox: PathBuf,
}

impl Locations {
    pub fn discover() -> Result<Self, AppError> {
        Ok(Self {
            firefox: firefox_config_dir(std::env::consts::OS, |key| {
                std::env::var_os(key).map(PathBuf::from)
            })?,
        })
    }
}

pub(crate) fn firefox_config_dir(
    os: &str,
    env: impl Fn(&str) -> Option<PathBuf>,
) -> Result<PathBuf, AppError> {
    let env_path = |key: &str| {
        env(key)
            .filter(|value| !value.as_os_str().is_empty())
            .ok_or_else(|| {
                AppError::new(
                    "local_paths_missing",
                    "Cannot locate local application directories.",
                    "Set up Firefox and the normal OS user directories. Do not pass a cookie as an argument.",
                )
            })
    };
    match os {
        "windows" => Ok(env_path("APPDATA")?.join("Mozilla").join("Firefox")),
        "macos" => Ok(env_path("HOME")?
            .join("Library")
            .join("Application Support")
            .join("Firefox")),
        _ => Ok(env_path("HOME")?.join(".mozilla").join("firefox")),
    }
}

pub(crate) fn executable_candidates(
    os: &str,
    env: impl Fn(&str) -> Option<PathBuf>,
) -> Vec<PathBuf> {
    match os {
        "windows" => ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"]
            .into_iter()
            .filter_map(|key| env(key).filter(|value| !value.as_os_str().is_empty()))
            .map(|root| root.join("Mozilla Firefox").join("firefox.exe"))
            .collect(),
        "macos" => {
            let mut paths = vec![PathBuf::from(
                "/Applications/Firefox.app/Contents/MacOS/firefox",
            )];
            if let Some(home) = env("HOME").filter(|value| !value.as_os_str().is_empty()) {
                paths.push(
                    home.join("Applications")
                        .join("Firefox.app")
                        .join("Contents")
                        .join("MacOS")
                        .join("firefox"),
                );
            }
            paths
        }
        _ => vec![
            PathBuf::from("/usr/bin/firefox"),
            PathBuf::from("/usr/lib/firefox/firefox"),
        ],
    }
}

pub(crate) fn firefox_executable_present() -> bool {
    executable_candidates(std::env::consts::OS, |key| {
        std::env::var_os(key).map(PathBuf::from)
    })
    .iter()
    .any(|path| path.is_file())
}

pub(crate) struct Secret(Zeroizing<String>);

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

impl Secret {
    fn new(value: String) -> Result<Self, AppError> {
        if !token_shape(&value) {
            return Err(AppError::new(
                "session_invalid",
                "The stored Splice session cannot be used.",
                "Open splice.com in Firefox and sign in again. Do not paste the cookie into the terminal.",
            ));
        }
        Ok(Self(Zeroizing::new(value)))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

fn token_shape(value: &str) -> bool {
    let mut parts = value.split('.');
    let Some(header) = parts.next() else {
        return false;
    };
    let Some(payload) = parts.next() else {
        return false;
    };
    let Some(signature) = parts.next() else {
        return false;
    };
    parts.next().is_none()
        && !header.is_empty()
        && !payload.is_empty()
        && !signature.is_empty()
        && value.len() <= 8_192
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

pub(crate) struct Session {
    pub profile: String,
    token: Secret,
}

impl fmt::Debug for Session {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Session")
            .field("profile", &self.profile)
            .field("token", &self.token)
            .finish()
    }
}

impl Session {
    pub fn authorization(&self) -> String {
        format!("Bearer {}", self.token.as_str())
    }
}

type Section = BTreeMap<String, String>;

fn ini(file: &Path) -> Result<Vec<Section>, AppError> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) if text.len() <= 65_536 => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        _ => {
            return Err(AppError::new(
                "profile_config_unreadable",
                "Cannot read the Firefox profile configuration.",
                "Check the local Firefox profile settings.",
            ));
        }
    };
    let mut sections: Vec<Section> = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with([';', '#']) {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            let mut section = Section::new();
            section.insert("section".to_owned(), name.to_owned());
            sections.push(section);
        } else if let Some((key, value)) = line.split_once('=')
            && let Some(section) = sections.last_mut()
        {
            section.insert(key.trim().to_owned(), value.trim().to_owned());
        }
    }
    Ok(sections)
}

pub(crate) fn profile_dir(
    root: &Path,
    requested: Option<&str>,
) -> Result<(PathBuf, String), AppError> {
    let config = ini(&root.join("profiles.ini"))?;
    let profiles: Vec<_> = config
        .iter()
        .filter(|section| {
            section
                .get("section")
                .is_some_and(|name| name.starts_with("Profile"))
                && section.contains_key("Path")
        })
        .collect();
    let matches: Vec<_> = if let Some(name) = requested {
        profiles
            .into_iter()
            .filter(|section| section.get("Name").is_some_and(|value| value == name))
            .collect()
    } else {
        let installs = ini(&root.join("installs.ini"))?;
        let mut defaults: BTreeSet<_> = installs
            .iter()
            .filter_map(|section| section.get("Default").cloned())
            .collect();
        if defaults.is_empty() {
            defaults.extend(
                config
                    .iter()
                    .filter(|section| {
                        section
                            .get("section")
                            .is_some_and(|name| name.starts_with("Install"))
                    })
                    .filter_map(|section| section.get("Default").cloned()),
            );
        }
        if defaults.len() > 1 {
            return Err(ambiguous_profile());
        }
        profiles
            .into_iter()
            .filter(|section| match defaults.first() {
                Some(default) => section.get("Path") == Some(default),
                None => section.get("Default").is_some_and(|value| value == "1"),
            })
            .collect()
    };
    if matches.len() != 1 {
        return Err(ambiguous_profile());
    }
    let selected = matches[0];
    let directory = if selected.get("IsRelative").is_some_and(|value| value == "1") {
        root.join(selected["Path"].replace('\\', "/"))
    } else {
        PathBuf::from(&selected["Path"])
    };
    if !directory.is_absolute() {
        return Err(ambiguous_profile());
    }
    let name = selected
        .get("Name")
        .cloned()
        .ok_or_else(ambiguous_profile)?;
    Ok((directory, name))
}

fn ambiguous_profile() -> AppError {
    AppError::new(
        "firefox_profile_ambiguous",
        "Cannot select exactly one Firefox profile.",
        "Use --profile with an existing Firefox profile name. Other profiles are not scanned.",
    )
}

pub(crate) fn read_session(
    root: &Path,
    requested: Option<&str>,
    now_seconds: u64,
) -> Result<Session, AppError> {
    let now_seconds_i64 = i64::try_from(now_seconds).map_err(|_| {
        AppError::new(
            "clock_invalid",
            "The current time is outside the supported range.",
            "Check the OS clock before a live request.",
        )
    })?;
    let (directory, profile) = profile_dir(root, requested)?;
    let database = directory.join("cookies.sqlite");
    let read_error = || {
        AppError::new(
            "cookie_read_failed",
            "Cannot read the Firefox cookie database read-only.",
            "Check that the selected Firefox profile exists and is readable. If Firefox has the database locked, close it and run once. The database is not copied.",
        )
    };
    let connection = Connection::open_with_flags(
        &database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| read_error())?;
    connection
        .busy_timeout(Duration::from_millis(250))
        .map_err(|_| read_error())?;
    let mut query = connection
        .prepare(
            "SELECT value FROM moz_cookies
             WHERE name = '_splice_token_prod'
               AND host IN ('.splice.com', 'splice.com')
               AND path = '/'
               AND originAttributes = ''
               AND (expiry = 0 OR expiry > ?1)",
        )
        .map_err(|_| read_error())?;
    let rows = query
        .query_map(params![now_seconds_i64], |row| row.get::<_, String>(0))
        .map_err(|_| read_error())?;
    let mut tokens = Vec::new();
    for row in rows {
        tokens.push(row.map_err(|_| read_error())?);
    }
    if tokens.len() != 1 {
        let code = if tokens.is_empty() {
            "session_missing"
        } else {
            "session_ambiguous"
        };
        return Err(AppError::new(
            code,
            "No unique unexpired Splice session was found in the selected Firefox profile.",
            "Open splice.com in that Firefox profile and sign in. Container tabs are not selected.",
        ));
    }
    let token = Secret::new(tokens.pop().unwrap())?;
    if jwt_expiry(token.as_str()).is_some_and(|exp| exp <= now_seconds) {
        return Err(AppError::new(
            "session_expired",
            "The Firefox Splice session has expired.",
            "Open splice.com in that Firefox profile so the session refreshes, then run once.",
        ));
    }
    Ok(Session { profile, token })
}

fn jwt_expiry(token: &str) -> Option<u64> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value.get("exp").and_then(serde_json::Value::as_u64)
}

pub(crate) fn firefox_present(root: &Path) -> bool {
    root.join("profiles.ini").is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("main")).unwrap();
        std::fs::write(
            root.path().join("profiles.ini"),
            "[Profile0]\nName=main\nIsRelative=1\nPath=main\nDefault=1\n",
        )
        .unwrap();
        let db = Connection::open(root.path().join("main/cookies.sqlite")).unwrap();
        db.execute_batch(
            "CREATE TABLE moz_cookies (name TEXT, value TEXT, host TEXT, path TEXT, originAttributes TEXT, expiry INTEGER);",
        )
        .unwrap();
        root
    }

    fn add(root: &Path, name: &str, host: &str, context: &str, value: &str, expiry: u64) {
        let db = Connection::open(root.join("main/cookies.sqlite")).unwrap();
        db.execute(
            "INSERT INTO moz_cookies VALUES (?1, ?2, ?3, '/', ?4, ?5)",
            params![name, value, host, context, i64::try_from(expiry).unwrap()],
        )
        .unwrap();
    }

    fn sample_token(exp: u64) -> String {
        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"none\"}");
        let payload = URL_SAFE_NO_PAD.encode(format!("{{\"exp\":{exp}}}").as_bytes());
        let signature = URL_SAFE_NO_PAD.encode(b"sig");
        format!("{header}.{payload}.{signature}")
    }

    #[test]
    fn reads_splice_token_without_printing_it() {
        let root = fixture();
        let token = sample_token(4_000_000_000);
        add(
            root.path(),
            "_splice_token_prod",
            ".splice.com",
            "",
            &token,
            4_000_000_000,
        );
        add(
            root.path(),
            "_splice_token_prod",
            ".example.com",
            "",
            &sample_token(4_000_000_000),
            4_000_000_000,
        );
        let session = read_session(root.path(), None, 1_000).unwrap();
        assert_eq!(session.profile, "main");
        assert!(!format!("{session:?}").contains(&token));
        assert!(session.authorization().starts_with("Bearer "));
    }

    #[test]
    fn rejects_expired_token() {
        let root = fixture();
        add(
            root.path(),
            "_splice_token_prod",
            ".splice.com",
            "",
            &sample_token(10),
            4_000_000_000,
        );
        assert_eq!(
            read_session(root.path(), None, 1_000).unwrap_err().code,
            "session_expired"
        );
    }
}
