//! Freedesktop sound-theme discovery and event resolution.
//!
//! It follows theme inheritance, the mandatory `freedesktop` fallback, sound
//! profiles (normally `stereo`), and `.disabled` event masks.

use crate::xdg::XdgPaths;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::PathBuf,
};

#[derive(Clone, Debug)]
pub struct SoundResolver {
    theme: String,
    paths: XdgPaths,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SoundRequest {
    pub event: String,
    pub profile: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SoundResolution {
    File(PathBuf),
    Disabled,
}

impl SoundRequest {
    pub fn stereo(event: impl Into<String>) -> Self {
        Self {
            event: event.into(),
            profile: "stereo".into(),
        }
    }
}
impl SoundResolver {
    pub fn new(theme: impl Into<String>) -> Self {
        Self {
            theme: theme.into(),
            paths: XdgPaths::default(),
        }
    }
    pub fn with_paths(theme: impl Into<String>, paths: XdgPaths) -> Self {
        Self {
            theme: theme.into(),
            paths,
        }
    }
    pub fn resolve(&self, request: &SoundRequest) -> Option<SoundResolution> {
        self.resolve_theme(&self.theme, request, &mut HashSet::new())
            .or_else(|| {
                (self.theme != "freedesktop")
                    .then(|| self.resolve_theme("freedesktop", request, &mut HashSet::new()))
                    .flatten()
            })
    }
    pub fn resolve_file(&self, request: &SoundRequest) -> Option<PathBuf> {
        match self.resolve(request)? {
            SoundResolution::File(path) => Some(path),
            SoundResolution::Disabled => None,
        }
    }
    fn resolve_theme(
        &self,
        theme: &str,
        request: &SoundRequest,
        visited: &mut HashSet<String>,
    ) -> Option<SoundResolution> {
        if !visited.insert(theme.into()) {
            return None;
        }
        for base in self.paths.data_search_paths() {
            let root = base.join("sounds").join(theme);
            let event = root.join(&request.profile).join(&request.event);
            if event.with_extension("disabled").is_file() {
                return Some(SoundResolution::Disabled);
            }
            for extension in ["oga", "ogg", "wav", "flac", "mp3"] {
                let path = event.with_extension(extension);
                if path.is_file() {
                    return Some(SoundResolution::File(path));
                }
            }
        }
        let inherits = self.theme_inherits(theme);
        inherits
            .into_iter()
            .find_map(|parent| self.resolve_theme(&parent, request, visited))
    }
    fn theme_inherits(&self, theme: &str) -> Vec<String> {
        self.paths
            .data_search_paths()
            .into_iter()
            .find_map(|base| {
                fs::read_to_string(base.join("sounds").join(theme).join("index.theme")).ok()
            })
            .map(|text| {
                parse_ini(&text)
                    .get("Sound Theme")
                    .and_then(|values| values.get("Inherits"))
                    .into_iter()
                    .flat_map(|value| value.split(','))
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }
}
pub fn system_sound_theme() -> String {
    let paths = XdgPaths::default();
    for path in [
        paths.config_home.join("gtk-4.0/settings.ini"),
        paths.config_home.join("gtk-3.0/settings.ini"),
    ] {
        if let Ok(source) = fs::read_to_string(path) {
            if let Some(theme) = parse_ini(&source)
                .get("Settings")
                .and_then(|values| values.get("gtk-sound-theme-name"))
                .filter(|value| !value.is_empty())
            {
                return theme.clone();
            }
        }
    }
    "freedesktop".into()
}
fn parse_ini(source: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut result = BTreeMap::new();
    let mut section: Option<String> = None;
    for line in source.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            section = Some(name.into());
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
    #[test]
    fn disabled_masks_inherited_sound() {
        let root = std::env::temp_dir().join(format!("delib-sounds-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("data/sounds/child/stereo")).unwrap();
        fs::create_dir_all(root.join("data/sounds/parent/stereo")).unwrap();
        fs::write(
            root.join("data/sounds/child/index.theme"),
            "[Sound Theme]\nInherits=parent\n",
        )
        .unwrap();
        fs::write(root.join("data/sounds/child/stereo/bell.disabled"), "").unwrap();
        fs::write(root.join("data/sounds/parent/stereo/bell.oga"), "").unwrap();
        let paths = XdgPaths {
            data_home: root.join("data"),
            data_dirs: vec![],
            config_home: root.join("config"),
            config_dirs: vec![],
        };
        assert_eq!(
            SoundResolver::with_paths("child", paths).resolve(&SoundRequest::stereo("bell")),
            Some(SoundResolution::Disabled)
        );
        let _ = fs::remove_dir_all(root);
    }
}
