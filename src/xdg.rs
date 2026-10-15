//! XDG Base Directory locations in the order in which they are searched.
//!
//! The type is deliberately just data: callers can construct it with fixture
//! directories in tests instead of changing process-wide environment variables.

use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XdgPaths {
    pub data_home: PathBuf,
    pub data_dirs: Vec<PathBuf>,
    pub config_home: PathBuf,
    pub config_dirs: Vec<PathBuf>,
}

impl XdgPaths {
    /// Reads the XDG base-directory environment variables and their defaults.
    pub fn from_env() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let data_home = absolute_env("XDG_DATA_HOME")
            .or_else(|| home.as_ref().map(|home| home.join(".local/share")))
            .unwrap_or_else(|| PathBuf::from(".local/share"));
        let config_home = absolute_env("XDG_CONFIG_HOME")
            .or_else(|| home.as_ref().map(|home| home.join(".config")))
            .unwrap_or_else(|| PathBuf::from(".config"));
        Self {
            data_home,
            data_dirs: split_absolute("XDG_DATA_DIRS", "/usr/local/share:/usr/share"),
            config_home,
            config_dirs: split_absolute("XDG_CONFIG_DIRS", "/etc/xdg"),
        }
    }

    /// Data roots, with the per-user location before system locations.
    pub fn data_search_paths(&self) -> Vec<PathBuf> {
        std::iter::once(self.data_home.clone())
            .chain(self.data_dirs.iter().cloned())
            .collect()
    }

    /// Configuration roots, with the per-user location before system locations.
    pub fn config_search_paths(&self) -> Vec<PathBuf> {
        std::iter::once(self.config_home.clone())
            .chain(self.config_dirs.iter().cloned())
            .collect()
    }

    /// Locations containing installed application desktop entries.
    pub fn application_dirs(&self) -> Vec<PathBuf> {
        self.data_search_paths()
            .into_iter()
            .map(|directory| directory.join("applications"))
            .collect()
    }
}

impl Default for XdgPaths {
    fn default() -> Self {
        Self::from_env()
    }
}

fn absolute_env(variable: &str) -> Option<PathBuf> {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

fn split_absolute(variable: &str, default: &str) -> Vec<PathBuf> {
    let value = std::env::var_os(variable).unwrap_or_else(|| default.into());
    let paths = std::env::split_paths(&value)
        .filter(|path| path.is_absolute())
        .collect::<Vec<_>>();
    if paths.is_empty() && std::env::var_os(variable).is_some() {
        std::env::split_paths(default).collect()
    } else {
        paths
    }
}

/// XDG data roots in user-to-system search order.
pub fn data_dirs() -> Vec<PathBuf> {
    XdgPaths::default().data_search_paths()
}
