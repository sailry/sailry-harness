//! Read-only Chrome import. Chromium's cookie database v24 and macOS v10 encryption:
//! net/extras/sqlite/sqlite_persistent_cookie_store.cc and
//! components/os_crypt/async/browser/keychain_key_provider.mm (main, 2026-09-16).
use rusqlite::{Connection, OpenFlags};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;
mod crypto;
#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(crate) struct Profile {
    pub name: String,
    pub database: PathBuf,
}

#[derive(Clone)]
pub(crate) struct Cookie {
    pub domain: String,
    pub name: String,
    pub value: Zeroizing<String>,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: i64,
    pub expires: Option<i64>,
}

pub(crate) struct Import {
    pub cookies: Vec<Cookie>,
    pub skipped: usize,
}

pub(crate) struct Prepared {
    cookies: Vec<PendingCookie>,
    skipped: usize,
}
#[derive(Clone)]
struct PendingCookie {
    cookie: Cookie,
    encrypted: Zeroizing<Vec<u8>>,
}
impl Prepared {
    pub(crate) fn needs_key(&self) -> bool {
        self.cookies
            .iter()
            .any(|cookie| !cookie.encrypted.is_empty())
    }
    pub(crate) fn unlock(&self) -> Result<Import> {
        self.decrypt(|| {
            security_framework::passwords::get_generic_password("Chrome Safe Storage", "Chrome")
                .map(Zeroizing::new)
                .map_err(|_| "browser_chrome_key_denied")
        })
    }
    fn decrypt(&self, key: impl FnOnce() -> Result<Zeroizing<Vec<u8>>>) -> Result<Import> {
        let secret = if self.needs_key() {
            Some(crypto::key(&key()?))
        } else {
            None
        };
        let mut result = Import {
            cookies: Vec::new(),
            skipped: self.skipped,
        };
        for mut pending in self.cookies.iter().cloned() {
            if !pending.encrypted.is_empty() {
                match crypto::decrypt(
                    secret.as_ref().unwrap(),
                    &pending.cookie.domain,
                    &pending.encrypted,
                ) {
                    Ok(value) => pending.cookie.value = value,
                    Err(_) => {
                        result.skipped += 1;
                        continue;
                    }
                }
            }
            result.cookies.push(pending.cookie);
        }
        Ok(result)
    }
}

pub(crate) type Result<T> = std::result::Result<T, &'static str>;

pub(crate) fn profiles() -> Result<Vec<Profile>> {
    let home = dirs::home_dir().ok_or("browser_chrome_missing")?;
    // Opening metadata after Import is pressed lets macOS request app-data
    // consent when required (WWDC23 10053); do not replace it with a file picker.
    discover(&home.join("Library/Application Support/Google/Chrome"))
}

pub(super) fn discover(root: &Path) -> Result<Vec<Profile>> {
    let data = std::fs::read(root.join("Local State")).map_err(discovery_error)?;
    let data: serde_json::Value =
        serde_json::from_slice(&data).map_err(|_| "browser_chrome_failed")?;
    let info = data["profile"]["info_cache"]
        .as_object()
        .ok_or("browser_chrome_failed")?;
    let mut profiles = Vec::new();
    for (folder, info) in info {
        if !matches!(
            Path::new(folder)
                .components()
                .collect::<Vec<_>>()
                .as_slice(),
            [std::path::Component::Normal(_)]
        ) {
            continue;
        }
        let directory = root.join(folder);
        let database = cookie_path(&directory)?;
        if let Some(database) = database {
            profiles.push(Profile {
                name: info["name"].as_str().unwrap_or(folder).into(),
                database,
            });
        }
    }
    Ok(profiles)
}

fn discovery_error(error: std::io::Error) -> &'static str {
    match error.kind() {
        std::io::ErrorKind::NotFound => "browser_chrome_missing",
        std::io::ErrorKind::PermissionDenied => "browser_chrome_access_denied",
        _ => "browser_chrome_failed",
    }
}

fn cookie_path(directory: &Path) -> Result<Option<PathBuf>> {
    for path in [directory.join("Cookies"), directory.join("Network/Cookies")] {
        match std::fs::metadata(&path) {
            Ok(metadata) if metadata.is_file() => return Ok(Some(path)),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(discovery_error(error)),
        }
    }
    Ok(None)
}

fn database(path: &Path) -> Result<Connection> {
    // The database can be protected separately from Local State. Opening it
    // requests native consent and preserves access-denied errors before SQLite.
    let _access = std::fs::File::open(path).map_err(discovery_error)?;
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| "browser_chrome_failed")?;
    db.busy_timeout(std::time::Duration::from_secs(2))
        .map_err(|_| "browser_chrome_failed")?;
    let version: i64 = db
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key='version'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| "browser_chrome_failed")?;
    if version != 24 {
        return Err("browser_chrome_schema");
    }
    Ok(db)
}

pub(crate) fn sites(profile: &Profile) -> Result<Vec<String>> {
    let db = database(&profile.database)?;
    let mut query = db
        .prepare("SELECT DISTINCT host_key FROM cookies ORDER BY host_key")
        .map_err(|_| "browser_chrome_failed")?;
    query
        .query_map([], |row| row.get(0))
        .map_err(|_| "browser_chrome_failed")?
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| "browser_chrome_failed")
}

pub(crate) fn prepare(profile: &Profile, sites: &BTreeSet<String>) -> Result<Prepared> {
    let db = database(&profile.database)?;
    let mut query = db.prepare("SELECT host_key,name,value,CAST(encrypted_value AS BLOB),path,is_secure,is_httponly,samesite,expires_utc,has_expires,top_frame_site_key FROM cookies")
        .map_err(|_| "browser_chrome_failed")?;
    let mut rows = query.query([]).map_err(|_| "browser_chrome_failed")?;
    let mut result = Prepared {
        cookies: Vec::new(),
        skipped: 0,
    };
    while let Some(row) = rows.next().map_err(|_| "browser_chrome_failed")? {
        let domain: String = row.get(0).map_err(|_| "browser_chrome_failed")?;
        if !sites.contains(&domain) {
            continue;
        }
        let parsed = (|| -> rusqlite::Result<_> {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Vec<u8>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, bool>(5)?,
                row.get::<_, bool>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, bool>(9)?,
                row.get::<_, String>(10)?,
            ))
        })()
        .map_err(|_| "browser_chrome_failed")?;
        let (
            name,
            plain,
            encrypted,
            path,
            secure,
            http_only,
            same_site,
            expires,
            has_expires,
            partition,
        ) = parsed;
        let plain = Zeroizing::new(plain);
        let expires = has_expires.then_some(expires / 1_000_000 - 11_644_473_600);
        if !partition.is_empty()
            || expires.is_some_and(|time| time <= chrono::Utc::now().timestamp())
        {
            result.skipped += 1;
            continue;
        }
        if !encrypted.is_empty() && !encrypted.starts_with(b"v10") {
            result.skipped += 1;
            continue;
        }
        result.cookies.push(PendingCookie {
            cookie: Cookie {
                domain,
                name,
                value: plain,
                path,
                secure,
                http_only,
                same_site,
                expires,
            },
            encrypted: Zeroizing::new(encrypted),
        });
    }
    Ok(result)
}
