//! Freedesktop icon-theme discovery and resolution, without a UI dependency.
//!
//! [`IconResolver`] turns a semantic icon name such as `folder-documents` into
//! the best available icon file for a selected theme. It honours `index.theme`,
//! size and scale metadata, inherited themes, `hicolor`, and unthemed pixmaps.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};

#[derive(Clone)]
/// Resolves icon names against one Freedesktop icon theme.
pub struct IconResolver {
    theme: String,
    bases: Vec<PathBuf>,
    themes: Rc<RefCell<HashMap<String, Option<Theme>>>>,
}

/// A preferred sequence of icon names and its target logical pixel size.
pub struct IconRequest {
    /// Candidate names in priority order. Absolute image paths are accepted.
    pub names: Vec<String>,
    /// Requested logical icon size in pixels.
    pub size: u32,
    /// Output scale factor, normally `1` and `2` on HiDPI displays.
    pub scale: u32,
}

impl IconRequest {
    /// Creates a one-name request at scale `1`.
    pub fn named(name: impl Into<String>, size: u32) -> Self {
        Self {
            names: vec![name.into()],
            size,
            scale: 1,
        }
    }
}

impl IconResolver {
    /// Creates a resolver for a theme directory name, for example `breeze-dark`.
    pub fn new(theme: impl Into<String>) -> Self {
        let mut bases = std::env::var_os("HOME")
            .map(|home| vec![PathBuf::from(home).join(".icons")])
            .unwrap_or_default();
        bases.extend(
            xdg_data_directories()
                .into_iter()
                .map(|path| path.join("icons")),
        );
        Self {
            theme: theme.into(),
            bases,
            themes: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    /// Resolves the best matching icon path.
    ///
    /// Resolution tries absolute paths first, then the selected theme, its
    /// parents, `hicolor`, and finally unthemed pixmaps.
    pub fn resolve(&self, request: IconRequest) -> Option<PathBuf> {
        for name in &request.names {
            let path = Path::new(name);
            if path.is_absolute() && path.is_file() && is_icon(path) {
                return Some(path.into());
            }
        }
        let mut visited = HashSet::new();
        for name in &request.names {
            if let Some(path) = self.resolve_theme(&self.theme, name, &request, &mut visited) {
                return Some(path);
            }
        }
        if self.theme != "hicolor" {
            for name in &request.names {
                if let Some(path) = self.resolve_theme("hicolor", name, &request, &mut visited) {
                    return Some(path);
                }
            }
        }
        self.resolve_pixmaps(&request)
    }

    fn resolve_theme(
        &self,
        theme_name: &str,
        icon: &str,
        request: &IconRequest,
        visited: &mut HashSet<String>,
    ) -> Option<PathBuf> {
        if !visited.insert(theme_name.into()) {
            return None;
        }
        let theme = self.theme(theme_name)?;
        if let Some(path) = theme.find(theme_name, icon, request, &self.bases) {
            return Some(path);
        }
        theme
            .inherits
            .iter()
            .find_map(|parent| self.resolve_theme(parent, icon, request, visited))
    }

    fn theme(&self, name: &str) -> Option<Theme> {
        if let Some(cached) = self.themes.borrow().get(name).cloned() {
            return cached;
        }
        let theme = self
            .bases
            .iter()
            .map(|base| base.join(name).join("index.theme"))
            .find_map(|path| fs::read_to_string(path).ok())
            .map(|text| Theme::parse(&text));
        self.themes.borrow_mut().insert(name.into(), theme.clone());
        theme
    }

    fn resolve_pixmaps(&self, request: &IconRequest) -> Option<PathBuf> {
        for name in &request.names {
            let name = icon_stem(name);
            for data in xdg_data_directories() {
                for extension in ["svg", "png"] {
                    let path = data.join("pixmaps").join(format!("{name}.{extension}"));
                    if path.is_file() {
                        return Some(path);
                    }
                }
            }
            for extension in ["svg", "png"] {
                let path = Path::new("/usr/share/pixmaps").join(format!("{name}.{extension}"));
                if path.is_file() {
                    return Some(path);
                }
            }
        }
        None
    }
}

#[derive(Clone)]
struct Theme {
    inherits: Vec<String>,
    directories: Vec<Directory>,
}
#[derive(Clone)]
struct Directory {
    path: String,
    size: u32,
    scale: u32,
    min: u32,
    max: u32,
}

impl Theme {
    fn parse(text: &str) -> Self {
        let sections = ini(text);
        let root = sections.get("Icon Theme");
        let inherits = root
            .and_then(|values| values.get("Inherits"))
            .into_iter()
            .flat_map(|value| value.split(','))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect();
        let directories = root
            .into_iter()
            .flat_map(|root| [root.get("Directories"), root.get("ScaledDirectories")])
            .flatten()
            .flat_map(|value| value.split(','))
            .map(str::trim)
            .filter_map(|path| {
                let values = sections.get(path)?;
                let size: u32 = values.get("Size")?.parse().ok()?;
                let scale = values
                    .get("Scale")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(1);
                let (min, max) = match values.get("Type").map(String::as_str) {
                    Some("Scalable") => (
                        values
                            .get("MinSize")
                            .and_then(|value| value.parse().ok())
                            .unwrap_or(size),
                        values
                            .get("MaxSize")
                            .and_then(|value| value.parse().ok())
                            .unwrap_or(size),
                    ),
                    _ => {
                        let threshold = values
                            .get("Threshold")
                            .and_then(|value| value.parse().ok())
                            .unwrap_or(2);
                        (
                            size.saturating_sub(threshold),
                            size.saturating_add(threshold),
                        )
                    }
                };
                Some(Directory {
                    path: path.into(),
                    size,
                    scale,
                    min,
                    max,
                })
            })
            .collect();
        Self {
            inherits,
            directories,
        }
    }

    fn find(
        &self,
        theme: &str,
        icon: &str,
        request: &IconRequest,
        bases: &[PathBuf],
    ) -> Option<PathBuf> {
        let mut dirs = self.directories.iter().collect::<Vec<_>>();
        dirs.sort_by_key(|directory| directory.distance(request.size, request.scale.max(1)));
        for directory in dirs {
            for base in bases {
                for extension in ["svg", "png"] {
                    let path = base.join(theme).join(&directory.path).join(format!(
                        "{}.{}",
                        icon_stem(icon),
                        extension
                    ));
                    if path.is_file() {
                        return Some(path);
                    }
                }
            }
        }
        None
    }
}

impl Directory {
    fn distance(&self, size: u32, scale: u32) -> u32 {
        let requested = size.saturating_mul(scale);
        let min = self.min.saturating_mul(self.scale);
        let max = self.max.saturating_mul(self.scale);
        if requested < min {
            min - requested
        } else if requested > max {
            requested - max
        } else {
            self.size.saturating_mul(self.scale).abs_diff(requested) / 100
        }
    }
}

/// Returns the icon-theme name selected by the current desktop.
///
/// KDE/Plasma settings in `kdeglobals` take precedence; GTK settings are a
/// cross-desktop fallback. Returns `hicolor` if no desktop selection exists.
pub fn system_icon_theme() -> String {
    system_icon_theme_with_override(None)
}

/// Returns the system icon theme, unless a nonempty named environment variable
/// explicitly overrides it.
///
/// This is intended for application-specific development overrides such as
/// `DFILES_ICON_THEME=Papirus`; it does not introduce a new desktop standard.
pub fn system_icon_theme_with_override(variable: Option<&str>) -> String {
    if let Some(theme) = variable
        .and_then(|name| std::env::var(name).ok())
        .filter(|value| !value.trim().is_empty())
    {
        return theme;
    }
    let config = config_directory();
    for path in [
        config.join("kdeglobals"),
        config.join("kdedefaults/kdeglobals"),
    ] {
        if let Some(theme) = ini_value(&path, "Icons", "Theme") {
            return theme;
        }
    }
    for path in [
        config.join("gtk-4.0/settings.ini"),
        config.join("gtk-3.0/settings.ini"),
    ] {
        if let Some(theme) = ini_value(&path, "Settings", "gtk-icon-theme-name") {
            return theme;
        }
    }
    "hicolor".into()
}

/// Returns XDG data directories in user-to-system search order.
pub fn xdg_data_directories() -> Vec<PathBuf> {
    let mut paths = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .into_iter()
        .collect::<Vec<_>>();
    let system =
        std::env::var_os("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    paths.extend(std::env::split_paths(&system));
    paths
}

/// Lists installed, user-selectable icon themes.
///
/// Themes marked `Hidden=true` in `index.theme` are omitted.
pub fn installed_icon_themes() -> Vec<String> {
    let resolver = IconResolver::new("hicolor");
    let mut seen = HashSet::new();
    let mut themes = Vec::new();
    for base in resolver.bases {
        let Ok(entries) = fs::read_dir(base) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if seen.insert(name.clone())
                && fs::read_to_string(entry.path().join("index.theme"))
                    .ok()
                    .is_some_and(|text| {
                        !ini(&text).get("Icon Theme").is_some_and(|values| {
                            values
                                .get("Hidden")
                                .is_some_and(|value| value.eq_ignore_ascii_case("true"))
                        })
                    })
            {
                themes.push(name);
            }
        }
    }
    themes.sort();
    themes
}

fn config_directory() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
}
fn ini_value(path: &Path, section: &str, key: &str) -> Option<String> {
    ini(&fs::read_to_string(path).ok()?)
        .get(section)?
        .get(key)
        .cloned()
}
fn ini(text: &str) -> HashMap<String, HashMap<String, String>> {
    let mut result = HashMap::<String, HashMap<String, String>>::new();
    let mut section = None::<String>;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            section = Some(name.into());
        } else if let (Some(section), Some((key, value))) = (section.as_ref(), line.split_once('='))
        {
            result
                .entry(section.clone())
                .or_default()
                .insert(key.trim().into(), value.trim().into());
        }
    }
    result
}
fn is_icon(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "svg"
            )
        })
}
fn icon_stem(name: &str) -> &str {
    let path = Path::new(name);
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension)
            if matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "svg"
            ) =>
        {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or(name)
        }
        _ => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_inheritance_and_scaled_directories() {
        let theme = Theme::parse(
            "[Icon Theme]\nInherits=parent,hicolor\nDirectories=48x48/apps\nScaledDirectories=48x48@2/apps\n[48x48/apps]\nSize=48\nType=Fixed\n[48x48@2/apps]\nSize=48\nScale=2\nType=Fixed\n",
        );
        assert_eq!(theme.inherits, ["parent", "hicolor"]);
        assert_eq!(theme.directories.len(), 2);
        assert_eq!(theme.directories[1].distance(48, 2), 0);
    }

    #[test]
    fn keeps_dotted_icon_names_but_strips_image_extensions() {
        assert_eq!(icon_stem("com.example.Editor"), "com.example.Editor");
        assert_eq!(icon_stem("folder.svg"), "folder");
    }
}
