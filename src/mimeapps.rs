//! MIME application association lookup as specified by MIME Apps.

use crate::{
    desktop_entry::{find_application, installed_applications_in, DesktopEntry},
    xdg::XdgPaths,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
};

#[derive(Clone, Debug)]
pub struct MimeApps {
    paths: XdgPaths,
    desktops: Vec<String>,
}

impl MimeApps {
    pub fn new(paths: XdgPaths) -> Self {
        let desktops = std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .split(':')
            .filter(|desktop| !desktop.is_empty())
            .map(|desktop| desktop.to_ascii_lowercase())
            .collect();
        Self { paths, desktops }
    }
    pub fn system() -> Self {
        Self::new(XdgPaths::default())
    }
    pub fn with_desktops(mut self, desktops: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.desktops = desktops
            .into_iter()
            .map(|desktop| desktop.into().to_ascii_lowercase())
            .collect();
        self
    }
    /// Returns the first usable default application for a MIME type.
    pub fn default_application(&self, mime: &str) -> Option<DesktopEntry> {
        for file in self.association_files() {
            let Ok(source) = fs::read_to_string(file) else {
                continue;
            };
            let values = parse(&source);
            let Some(ids) = values
                .get("Default Applications")
                .and_then(|values| values.get(mime))
            else {
                continue;
            };
            for id in ids.split(';').filter(|id| !id.is_empty()) {
                if let Some(entry) = find_application(&self.paths, id)
                    .filter(|entry| entry.is_application() && !entry.hidden())
                {
                    return Some(entry);
                }
            }
        }
        None
    }

    /// Lists visible handlers in association preference order, honoring user
    /// additions, removals and desktop-file overrides.
    pub fn applications_for(&self, mime: &str) -> Vec<DesktopEntry> {
        let mut directories: Vec<_> = self
            .paths
            .config_search_paths()
            .into_iter()
            .map(|path| (path, false))
            .collect();
        directories.extend(
            self.paths
                .application_dirs()
                .into_iter()
                .map(|path| (path, true)),
        );
        let applications: Vec<_> = directories
            .iter()
            .map(|(directory, application_dir)| {
                if *application_dir {
                    installed_applications_in([directory.clone()])
                } else {
                    Vec::new()
                }
            })
            .collect();
        let mut blocked = BTreeSet::new();
        let mut results = Vec::new();
        for (index, (directory, application_dir)) in directories.iter().enumerate() {
            let source = fs::read_to_string(directory.join("mimeapps.list")).unwrap_or_default();
            let values = parse(&source);
            let ids = |group: &str| -> Vec<String> {
                values
                    .get(group)
                    .and_then(|group| group.get(mime))
                    .map(|value| {
                        value
                            .split(';')
                            .filter(|id| !id.is_empty())
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default()
            };
            for id in ids("Added Associations") {
                if !blocked.contains(&id) {
                    if let Some(entry) = applications[index..]
                        .iter()
                        .flatten()
                        .find(|entry| entry.id() == Some(id.as_str()))
                        .filter(|entry| entry.is_visible_application())
                    {
                        if !results
                            .iter()
                            .any(|existing: &DesktopEntry| existing.id() == entry.id())
                        {
                            results.push(entry.clone());
                        }
                    }
                }
            }
            blocked.extend(ids("Removed Associations"));
            if *application_dir {
                let local = &applications[index];
                for entry in local {
                    if entry.is_visible_application()
                        && entry.mime_types().contains(&mime)
                        && !entry.id().is_some_and(|id| blocked.contains(id))
                        && !results.iter().any(|existing| existing.id() == entry.id())
                    {
                        results.push(entry.clone());
                    }
                }
                blocked.extend(
                    local
                        .iter()
                        .filter_map(|entry| entry.id().map(str::to_owned)),
                );
            }
        }
        if let Some(default) = self
            .default_application(mime)
            .filter(|entry| entry.is_visible_application())
        {
            results.retain(|entry| entry.id() != default.id());
            results.insert(0, default);
        }
        results
    }

    /// Resolves a MIME type and prepares its default application's argv for
    /// the supplied files. The returned command can be spawned without a shell.
    pub fn command_for(
        &self,
        mime: &str,
        files: &[PathBuf],
    ) -> Option<(DesktopEntry, Vec<String>)> {
        let entry = self.default_application(mime)?;
        let command = entry.command_for_files(files)?;
        Some((entry, command))
    }
    fn association_files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let names = self
            .desktops
            .iter()
            .map(|desktop| format!("{desktop}-mimeapps.list"))
            .collect::<Vec<_>>();
        for directory in self.paths.config_search_paths() {
            files.extend(names.iter().map(|name| directory.join(name)));
            files.push(directory.join("mimeapps.list"));
        }
        for directory in self.paths.application_dirs() {
            files.extend(names.iter().map(|name| directory.join(name)));
            files.push(directory.join("mimeapps.list"));
        }
        files
    }
}
impl Default for MimeApps {
    fn default() -> Self {
        Self::system()
    }
}

fn parse(text: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut result = BTreeMap::new();
    let mut section = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            section = Some(name.to_owned());
        } else if let (Some(section), Some((key, value))) = (&section, line.split_once('=')) {
            result
                .entry(section.clone())
                .or_insert_with(BTreeMap::new)
                .insert(key.trim().into(), value.trim().into());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn handlers_honor_additions_removals_and_user_desktop_overrides() {
        let root = std::env::temp_dir().join(format!("delib-handlers-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for path in ["config", "user/applications", "system/applications"] {
            fs::create_dir_all(root.join(path)).unwrap();
        }
        for (path, mime) in [
            ("user/applications/preferred.desktop", "text/plain"),
            ("user/applications/override.desktop", "text/plain"),
            ("system/applications/removed.desktop", "text/plain"),
            ("system/applications/extra.desktop", "image/png"),
            ("system/applications/override.desktop", "image/png"),
        ] {
            fs::write(root.join(path), format!("[Desktop Entry]\nType=Application\nName=Handler\nExec=handler %F\nMimeType={mime};\n")).unwrap();
        }
        fs::write(root.join("config/mimeapps.list"), "[Added Associations]\ntext/plain=extra.desktop;\n[Removed Associations]\ntext/plain=removed.desktop;\n").unwrap();
        fs::write(root.join("config/coconut-mimeapps.list"), "[Default Applications]\ntext/plain=preferred.desktop;\n[Added Associations]\ntext/plain=removed.desktop;\n").unwrap();
        fs::write(
            root.join("system/applications/mimeapps.list"),
            "[Removed Associations]\ntext/plain=override.desktop;\n",
        )
        .unwrap();
        let paths = XdgPaths {
            config_home: root.join("config"),
            config_dirs: vec![],
            data_home: root.join("user"),
            data_dirs: vec![root.join("system")],
        };
        let applications = MimeApps::new(paths)
            .with_desktops(["Coconut"])
            .applications_for("text/plain");
        let ids: Vec<_> = applications.iter().filter_map(DesktopEntry::id).collect();
        assert_eq!(
            ids,
            vec!["preferred.desktop", "extra.desktop", "override.desktop"]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn uses_desktop_specific_config_before_generic() {
        let root = std::env::temp_dir().join(format!("delib-mimeapps-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("config")).unwrap();
        fs::create_dir_all(root.join("data/applications")).unwrap();
        fs::write(
            root.join("config/coconut-mimeapps.list"),
            "[Default Applications]\ntext/plain=editor.desktop;\n",
        )
        .unwrap();
        fs::write(
            root.join("data/applications/editor.desktop"),
            "[Desktop Entry]\nType=Application\nName=Editor\nExec=editor %F\n",
        )
        .unwrap();
        let paths = XdgPaths {
            data_home: root.join("data"),
            data_dirs: vec![],
            config_home: root.join("config"),
            config_dirs: vec![],
        };
        assert_eq!(
            MimeApps::new(paths)
                .with_desktops(["Coconut"])
                .default_application("text/plain")
                .unwrap()
                .id(),
            Some("editor.desktop")
        );
        let _ = fs::remove_dir_all(root);
    }
}
