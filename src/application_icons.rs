//! Resolving application/window icons from Freedesktop desktop entries.
//!
//! This module deliberately returns filesystem paths. Rendering toolkits and
//! compositors keep ownership of image decoding, scaling, and their caches.

use crate::{
    desktop_entry::installed_applications_in,
    icons::{IconRequest, IconResolver},
    xdg::XdgPaths,
};
use std::{collections::HashMap, path::PathBuf};

/// Resolves an application ID (for example a Wayland `app_id`) to its icon.
#[derive(Clone)]
pub struct ApplicationIconResolver {
    applications: HashMap<String, Option<String>>,
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
                Some((id, entry.icon().map(str::to_owned)))
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
        self.resolve_with_extensions(application_id, size, scale, &["svg", "png"])
    }

    /// Like [`Self::resolve`], but returns only raster image files.
    pub fn resolve_raster(&self, application_id: &str, size: u32, scale: u32) -> Option<PathBuf> {
        self.resolve_with_extensions(application_id, size, scale, &["png", "jpg", "jpeg"])
    }

    /// Resolves supported formats in caller preference order within each matching directory.
    pub fn resolve_with_extensions(
        &self,
        application_id: &str,
        size: u32,
        scale: u32,
        extensions: &[&str],
    ) -> Option<PathBuf> {
        let mut names = self
            .icon_for(application_id)
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
        self.icons.resolve_with_extensions(request, extensions)
    }

    fn icon_for(&self, application_id: &str) -> Option<&str> {
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
            .and_then(Option::as_deref)
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

    #[test]
    fn retains_only_icon_values_and_honors_desktop_entry_precedence() {
        let root =
            std::env::temp_dir().join(format!("delib-app-icon-metadata-{}", std::process::id()));
        let mut paths = paths(&root);
        paths.data_dirs.push(root.join("system"));
        let user = paths.data_home.join("applications");
        let system = paths.data_dirs[0].join("applications");
        fs::create_dir_all(&user).unwrap();
        fs::create_dir_all(&system).unwrap();
        let mut metadata = "[Desktop Entry]\nType=Application\nIcon=chosen\n".to_owned();
        for index in 0..1000 {
            metadata.push_str(&format!("Name[x-{index}]={}\n", "x".repeat(1024)));
        }
        fs::write(user.join("large.desktop"), metadata).unwrap();
        fs::write(
            user.join("empty.desktop"),
            "[Desktop Entry]\nType=Application\nName=User\n",
        )
        .unwrap();
        fs::write(
            system.join("empty.desktop"),
            "[Desktop Entry]\nType=Application\nIcon=system-only\n",
        )
        .unwrap();
        let resolver = ApplicationIconResolver::with_paths(&paths, "hicolor");
        assert_eq!(
            resolver.applications["large.desktop"].as_deref(),
            Some("chosen")
        );
        assert_eq!(resolver.applications["empty.desktop"], None);
        let retained: usize = ["large.desktop", "empty.desktop"]
            .into_iter()
            .map(|id| resolver.applications.get_key_value(id).unwrap())
            .map(|(id, icon)| id.capacity() + icon.as_ref().map_or(0, String::capacity))
            .sum();
        assert!(retained < 128);
        assert_eq!(resolver.icon_for("large"), Some("chosen"));
        assert_eq!(resolver.icon_for("large.desktop"), Some("chosen"));
        assert_eq!(resolver.icon_for("empty"), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn requested_formats_preserve_theme_priority_and_format_preferences() {
        let root =
            std::env::temp_dir().join(format!("delib-app-icon-formats-{}", std::process::id()));
        let paths = paths(&root);
        let apps = paths.data_home.join("applications");
        fs::create_dir_all(&apps).unwrap();
        fs::write(
            apps.join("test.desktop"),
            "[Desktop Entry]\nType=Application\nIcon=chosen\n",
        )
        .unwrap();
        for (theme, extra) in [("Child", "Inherits=Parent\n"), ("Parent", "")] {
            let theme = paths.data_home.join("icons").join(theme);
            fs::create_dir_all(theme.join("24/apps")).unwrap();
            fs::write(theme.join("index.theme"), format!("[Icon Theme]\nName=Test\n{extra}Directories=24/apps\n[24/apps]\nSize=24\nType=Fixed\n")).unwrap();
        }
        let child = paths.data_home.join("icons/Child/24/apps");
        fs::write(child.join("chosen.jpeg"), b"jpeg").unwrap();
        fs::write(
            paths.data_home.join("icons/Parent/24/apps/chosen.svg"),
            b"svg",
        )
        .unwrap();
        let resolver = ApplicationIconResolver::with_paths(&paths, "Child");
        let formats = &["svg", "png", "jpg", "jpeg"];
        assert_eq!(
            resolver.resolve_with_extensions("test", 24, 1, formats),
            Some(child.join("chosen.jpeg"))
        );
        fs::write(child.join("chosen.svg"), b"svg").unwrap();
        assert_eq!(
            resolver.resolve_with_extensions("test", 24, 1, formats),
            Some(child.join("chosen.svg"))
        );
        assert_eq!(
            resolver.resolve_raster("test", 24, 1),
            Some(child.join("chosen.jpeg"))
        );
        assert_eq!(resolver.resolve_with_extensions("test", 24, 1, &[]), None);
        fs::write(
            apps.join("fallback.desktop"),
            "[Desktop Entry]\nType=Application\nIcon=not-installed\n",
        )
        .unwrap();
        fs::write(child.join("fallback.png"), b"png").unwrap();
        let resolver = ApplicationIconResolver::with_paths(&paths, "Child");
        assert_eq!(
            resolver.resolve_with_extensions("fallback", 24, 1, formats),
            Some(child.join("fallback.png"))
        );
        fs::remove_dir_all(root).unwrap();
    }
}
