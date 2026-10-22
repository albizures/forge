//! Durable local foundations for the independent Forgeup executable.
//!
//! This crate owns Forgeup's private filesystem layout and schema-versioned
//! registry. Higher-level registration and daemon operations build on this
//! boundary rather than writing files directly.

use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    env,
    ffi::OsString,
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const REGISTRY_SCHEMA_VERSION: u32 = 1;
const REGISTRY_FILE: &str = "registry.toml";
const LOCK_FILE: &str = "registry.lock";
static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// All user-owned locations used by Forgeup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForgeupPaths {
    /// Persistent configuration, including the registry.
    pub config: PathBuf,
    /// Immutable revision installations and durable revision state.
    pub data: PathBuf,
    /// Ephemeral protected process records.
    pub runtime: PathBuf,
    /// Directory containing the manager-owned `forge` path entry.
    pub bin: PathBuf,
}

impl ForgeupPaths {
    /// Resolves the Forgeup layout from `FORGEUP_HOME` or platform conventions.
    ///
    /// # Errors
    ///
    /// Returns an error when a required home or platform location is unavailable.
    pub fn from_environment() -> Result<Self, ForgeupError> {
        if let Some(home) = env::var_os("FORGEUP_HOME") {
            let home = PathBuf::from(home);
            if home.as_os_str().is_empty() {
                return Err(ForgeupError::InvalidLocation("FORGEUP_HOME is empty"));
            }
            return Ok(Self {
                config: home.join("config"),
                data: home.join("data"),
                runtime: home.join("runtime"),
                bin: home.join("bin"),
            });
        }

        let home = home_dir()?;
        #[cfg(target_os = "windows")]
        {
            let roaming = variable_path("APPDATA")?;
            let local = variable_path("LOCALAPPDATA")?;
            return Ok(Self {
                config: roaming.join("Forgeup"),
                data: local.join("Forgeup"),
                runtime: local.join("Forgeup").join("runtime"),
                bin: local.join("Forgeup").join("bin"),
            });
        }
        #[cfg(target_os = "macos")]
        {
            return Ok(Self {
                config: home.join("Library/Application Support/Forgeup"),
                data: home.join("Library/Application Support/Forgeup"),
                runtime: home.join("Library/Caches/Forgeup/runtime"),
                bin: home.join("Library/Application Support/Forgeup/bin"),
            });
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let config = env_path("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
            let data = env_path("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share"));
            let runtime =
                env_path("XDG_RUNTIME_DIR").unwrap_or_else(|| data.join("forgeup/runtime"));
            Ok(Self {
                config: config.join("forgeup"),
                data: data.join("forgeup"),
                runtime: runtime.join("forgeup"),
                bin: data.join("forgeup/bin"),
            })
        }
    }

    /// Creates and verifies the private directory layout.
    ///
    /// # Errors
    ///
    /// Returns an error if locations cannot be created or are accessible by other users.
    pub fn ensure_private(&self) -> Result<(), ForgeupError> {
        for path in [&self.config, &self.data, &self.runtime, &self.bin] {
            fs::create_dir_all(path).map_err(|source| ForgeupError::Io {
                path: path.clone(),
                source,
            })?;
            set_private_permissions(path)?;
            verify_private_permissions(path)?;
        }
        Ok(())
    }

    /// Returns the registry path.
    #[must_use]
    pub fn registry(&self) -> PathBuf {
        self.config.join(REGISTRY_FILE)
    }

    fn lock(&self) -> PathBuf {
        self.config.join(LOCK_FILE)
    }
}

/// A strictly validated persisted Forgeup registry.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    /// On-disk schema revision.
    pub schema_version: u32,
}

impl Registry {
    fn empty() -> Self {
        Self {
            schema_version: REGISTRY_SCHEMA_VERSION,
        }
    }
}

/// Holds the cross-process registry lock while reading or writing the registry.
pub struct RegistryStore {
    paths: ForgeupPaths,
    _lock: File,
}

impl RegistryStore {
    /// Opens the registry and obtains its exclusive cross-process lock.
    ///
    /// # Errors
    ///
    /// Returns an error for unsafe filesystem permissions or an unavailable lock.
    pub fn open(paths: ForgeupPaths) -> Result<Self, ForgeupError> {
        paths.ensure_private()?;
        let lock_path = paths.lock();
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|source| ForgeupError::Io {
                path: lock_path.clone(),
                source,
            })?;
        set_private_permissions(&lock_path)?;
        verify_private_permissions(&lock_path)?;
        lock.lock_exclusive().map_err(|source| ForgeupError::Io {
            path: lock_path,
            source,
        })?;
        Ok(Self { paths, _lock: lock })
    }

    /// Loads the registry, migrating older supported schemas transactionally.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed, unsupported, or unreadable registry data.
    pub fn load(&self) -> Result<Registry, ForgeupError> {
        let path = self.paths.registry();
        if !path.exists() {
            return Ok(Registry::empty());
        }
        verify_private_permissions(&path)?;
        let contents = fs::read_to_string(&path).map_err(|source| ForgeupError::Io {
            path: path.clone(),
            source,
        })?;
        let registry = toml::from_str::<Registry>(&contents).map_err(|source| {
            ForgeupError::InvalidRegistry {
                path: path.clone(),
                message: source.to_string(),
            }
        })?;
        match registry.schema_version {
            REGISTRY_SCHEMA_VERSION => Ok(registry),
            version if version > REGISTRY_SCHEMA_VERSION => {
                Err(ForgeupError::FutureSchema(version))
            }
            version => self.migrate(version, &contents),
        }
    }

    /// Atomically replaces the registry while the caller holds the lock.
    ///
    /// # Errors
    ///
    /// Returns an error when serialization or filesystem replacement fails.
    pub fn save(&self, registry: &Registry) -> Result<(), ForgeupError> {
        if registry.schema_version != REGISTRY_SCHEMA_VERSION {
            return Err(ForgeupError::UnsupportedSchema(registry.schema_version));
        }
        let contents =
            toml::to_string_pretty(registry).map_err(|source| ForgeupError::InvalidRegistry {
                path: self.paths.registry(),
                message: source.to_string(),
            })?;
        atomic_write(&self.paths.registry(), contents.as_bytes())
    }

    fn migrate(&self, version: u32, old_contents: &str) -> Result<Registry, ForgeupError> {
        if version != 0 {
            return Err(ForgeupError::UnsupportedSchema(version));
        }
        let backup = self.paths.registry().with_extension("toml.bak");
        atomic_write(&backup, old_contents.as_bytes())?;
        let registry = Registry::empty();
        self.save(&registry)?;
        Ok(registry)
    }
}

/// A public, non-mutating diagnosis of Forgeup's foundational storage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DoctorReport {
    /// Resolved user-owned locations.
    pub paths: ForgeupPaths,
    /// The registry schema currently usable by this Forgeup build.
    pub schema_version: u32,
}

/// Verifies the private layout and registry without implementing daemon control.
///
/// # Errors
///
/// Returns an error that identifies unsafe or unreadable foundational state.
pub fn doctor() -> Result<DoctorReport, ForgeupError> {
    let paths = ForgeupPaths::from_environment()?;
    let registry = RegistryStore::open(paths.clone())?.load()?;
    Ok(DoctorReport {
        paths,
        schema_version: registry.schema_version,
    })
}

/// Errors from Forgeup's local persistence boundary.
#[derive(Debug)]
pub enum ForgeupError {
    /// A required environment-derived location was absent or invalid.
    InvalidLocation(&'static str),
    /// An I/O operation failed at a specific path.
    Io { path: PathBuf, source: io::Error },
    /// A directory or secret-bearing file is not private to its owner.
    UnsafePermissions(PathBuf),
    /// TOML was malformed or violated the strict schema.
    InvalidRegistry { path: PathBuf, message: String },
    /// The registry was written by a newer Forgeup.
    FutureSchema(u32),
    /// An older registry schema has no declared migration.
    UnsupportedSchema(u32),
}

impl fmt::Display for ForgeupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLocation(message) => {
                write!(formatter, "invalid Forgeup location: {message}")
            }
            Self::Io { path, source } => write!(formatter, "{}: {source}", path.display()),
            Self::UnsafePermissions(path) => {
                write!(formatter, "{} must be private to its owner", path.display())
            }
            Self::InvalidRegistry { path, message } => {
                write!(formatter, "invalid registry {}: {message}", path.display())
            }
            Self::FutureSchema(version) => write!(
                formatter,
                "registry schema {version} is newer than this Forgeup"
            ),
            Self::UnsupportedSchema(version) => {
                write!(formatter, "registry schema {version} is unsupported")
            }
        }
    }
}

impl std::error::Error for ForgeupError {}

fn home_dir() -> Result<PathBuf, ForgeupError> {
    env_path("HOME").ok_or(ForgeupError::InvalidLocation("HOME is not set"))
}

#[cfg(target_os = "windows")]
fn variable_path(name: &'static str) -> Result<PathBuf, ForgeupError> {
    env_path(name).ok_or(ForgeupError::InvalidLocation(name))
}

fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), ForgeupError> {
    let parent = path
        .parent()
        .ok_or(ForgeupError::InvalidLocation("registry has no parent"))?;
    let temporary = parent.join(format!(
        ".{}.tmp-{}-{}",
        file_name(path).to_string_lossy(),
        std::process::id(),
        TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|source| ForgeupError::Io {
            path: temporary.clone(),
            source,
        })?;
    set_private_permissions(&temporary)?;
    file.write_all(contents)
        .map_err(|source| ForgeupError::Io {
            path: temporary.clone(),
            source,
        })?;
    file.sync_all().map_err(|source| ForgeupError::Io {
        path: temporary.clone(),
        source,
    })?;
    fs::rename(&temporary, path).map_err(|source| ForgeupError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| ForgeupError::Io {
            path: parent.to_path_buf(),
            source,
        })
}

fn file_name(path: &Path) -> OsString {
    path.file_name().unwrap_or_default().to_os_string()
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> Result<(), ForgeupError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|source| {
        ForgeupError::Io {
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) -> Result<(), ForgeupError> {
    Ok(())
}

#[cfg(unix)]
fn verify_private_permissions(path: &Path) -> Result<(), ForgeupError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(path)
        .map_err(|source| ForgeupError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        return Err(ForgeupError::UnsafePermissions(path.to_path_buf()));
    }
    Ok(())
}

#[cfg(not(unix))]
fn verify_private_permissions(_path: &Path) -> Result<(), ForgeupError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Arc, thread};

    fn paths(name: &str) -> ForgeupPaths {
        let root = env::temp_dir().join(format!("forgeup-test-{name}-{}", std::process::id()));
        ForgeupPaths {
            config: root.join("config"),
            data: root.join("data"),
            runtime: root.join("runtime"),
            bin: root.join("bin"),
        }
    }

    #[test]
    fn isolated_layout_is_private_and_registry_round_trips() {
        let paths = paths("round-trip");
        let store = RegistryStore::open(paths.clone()).unwrap();
        let registry = store.load().unwrap();
        assert_eq!(registry, Registry::empty());
        store.save(&registry).unwrap();
        drop(store);
        assert_eq!(
            RegistryStore::open(paths).unwrap().load().unwrap(),
            registry
        );
    }

    #[test]
    fn future_schema_is_rejected_without_writing() {
        let paths = paths("future");
        paths.ensure_private().unwrap();
        fs::write(paths.registry(), "schema_version = 2\n").unwrap();
        set_private_permissions(&paths.registry()).unwrap();
        assert!(matches!(
            RegistryStore::open(paths).unwrap().load(),
            Err(ForgeupError::FutureSchema(2))
        ));
    }

    #[test]
    fn schema_zero_is_backed_up_before_migration() {
        let paths = paths("migration");
        paths.ensure_private().unwrap();
        fs::write(paths.registry(), "schema_version = 0\n").unwrap();
        set_private_permissions(&paths.registry()).unwrap();
        let registry = RegistryStore::open(paths.clone()).unwrap().load().unwrap();
        assert_eq!(registry, Registry::empty());
        assert_eq!(
            fs::read_to_string(paths.registry().with_extension("toml.bak")).unwrap(),
            "schema_version = 0\n"
        );
    }

    #[test]
    fn exclusive_lock_serializes_registry_access() {
        let paths = Arc::new(paths("lock"));
        let held = RegistryStore::open((*paths).clone()).unwrap();
        let other_paths = Arc::clone(&paths);
        let waiting = thread::spawn(move || RegistryStore::open((*other_paths).clone()).unwrap());
        thread::sleep(std::time::Duration::from_millis(30));
        drop(held);
        drop(waiting.join().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn group_readable_registry_is_rejected() {
        use std::os::unix::fs::PermissionsExt;
        let paths = paths("permissions");
        paths.ensure_private().unwrap();
        fs::write(paths.registry(), "schema_version = 1\n").unwrap();
        fs::set_permissions(paths.registry(), fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            RegistryStore::open(paths).unwrap().load(),
            Err(ForgeupError::UnsafePermissions(_))
        ));
    }
}
