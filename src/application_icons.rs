//! Resolving application/window icons from Freedesktop desktop entries.
//!
//! This module deliberately returns filesystem paths. Rendering toolkits and
//! compositors keep ownership of image decoding, scaling, and their caches.

use crate::{
    desktop_entry::{installed_applications_in, DesktopEntry},
    icons::{IconRequest, IconResolver},
    xdg::XdgPaths,
};
use std::{collections::HashMap, path::PathBuf};

/// Resolves an application ID (for example a Wayland `app_id`) to its icon.
#[derive(Clone)]
pub struct ApplicationIconResolver {
    applications: HashMap<String, DesktopEntry>,
    icons: IconResolver,
}

impl ApplicationIconResolver {
    /// Uses the current desktop's installed applications and the named icon
    /// theme. Construct one resolver per selected theme and retain it.
    pub fn new(theme: impl Into<String>) -> Self {
        let paths = XdgPaths::default();
        Self::from_parts(&paths, IconResolver::new(theme))
    }

    /// Uses explicit XDG paths for deterministic discovery.
    pub fn with_paths(paths: &XdgPaths, theme: impl Into<String>) -> Self {
        Self::from_parts(paths, IconResolver::with_paths(paths, theme))
    }

    fn from_parts(paths: &XdgPaths, icons: IconResolver) -> Self {
        let applications = installed_applications_in(application_dirs(paths))
            .into_iter()
            .filter_map(|entry| {
                let id = entry.id()?.to_owned();
                Some((id, entry))
            })
            .collect();
        Self {
            applications,
            icons,
        }
    }

    /// Resolves the best icon file for an application. The desktop entry's
    /// `Icon` value has priority; the app ID itself is used as a practical
    /// fallback for clients that do not ship a desktop entry.
    pub fn resolve(&self, application_id: &str, size: u32, scale: u32) -> Option<PathBuf> {
        self.resolve_inner(application_id, size, scale, false)
    }

    /// Like [`Self::resolve`], but returns only raster image files.
    pub fn resolve_raster(&self, application_id: &str, size: u32, scale: u32) -> Option<PathBuf> {
        self.resolve_inner(application_id, size, scale, true)
    }

    fn resolve_inner(
        &self,
        application_id: &str,
        size: u32,
        scale: u32,
        raster: bool,
    ) -> Option<PathBuf> {
        let mut names = self
            .entry_for(application_id)
            .and_then(|entry| entry.icon())
            .filter(|icon| !icon.trim().is_empty())
            .map(str::to_owned)
            .into_iter()
            .collect::<Vec<_>>();
        for candidate in application_id_candidates(application_id) {
            if !names.contains(&candidate) {
                names.push(candidate);
            }
        }
        let request = IconRequest {
            names,
            size,
            scale: scale.max(1),
        };
        if raster {
            self.icons.resolve_raster(request)
        } else {
            self.icons.resolve(request)
        }
    }

    fn entry_for(&self, application_id: &str) -> Option<&DesktopEntry> {
        application_id_candidates(application_id)
            .into_iter()
            .find_map(|candidate| {
                let id = if candidate.ends_with(".desktop") {
                    candidate
                } else {
                    format!("{candidate}.desktop")
                };
                self.applications.get(&id)
            })
    }
}

fn application_dirs(paths: &XdgPaths) -> Vec<PathBuf> {
    let mut directories = vec![paths.data_home.join("applications")];
    directories.push(paths.data_home.join("flatpak/exports/share/applications"));
    for data_dir in &paths.data_dirs {
        directories.push(data_dir.join("applications"));
    }
    directories.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
    directories
}

fn application_id_candidates(application_id: &str) -> Vec<String> {
    let trimmed = application_id.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let without_suffix = trimmed.strip_suffix(".desktop").unwrap_or(trimmed);
    let mut candidates = vec![trimmed.to_owned(), without_suffix.to_owned()];
    let lowercase = without_suffix.to_ascii_lowercase();
    if !candidates.contains(&lowercase) {
        candidates.push(lowercase);
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn paths(root: &std::path::Path) -> XdgPaths {
        XdgPaths {
            data_home: root.join("data"),
            data_dirs: Vec::new(),
            config_home: root.join("config"),
            config_dirs: Vec::new(),
        }
    }

    #[test]
    fn resolves_the_desktop_entry_icon_for_an_app_id() {
        let root = std::env::temp_dir().join(format!("delib-app-icon-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let app_dir = root.join("data/applications");
        fs::create_dir_all(&app_dir).unwrap();
        let icon = root.join("app.png");
        fs::write(&icon, b"png").unwrap();
        fs::write(
            app_dir.join("org.example.App.desktop"),
            format!(
                "[Desktop Entry]\nType=Application\nIcon={}\n",
                icon.display()
            ),
        )
        .unwrap();
        let resolver = ApplicationIconResolver::with_paths(&paths(&root), "hicolor");
        assert_eq!(
            resolver.resolve("org.example.App", 32, 1),
            Some(icon.clone())
        );
        assert_eq!(
            resolver.resolve_raster("org.example.App.desktop", 32, 1),
            Some(icon)
        );
        fs::remove_dir_all(root).unwrap();
    }
}
