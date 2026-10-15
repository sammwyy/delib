//! Reading and launching [Desktop Entry] files.
//!
//! This module handles the Desktop Entry Specification's escaped values and
//! `Exec` field codes, but intentionally does not execute a shell.

use crate::xdg::XdgPaths;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopEntry {
    path: PathBuf,
    id: Option<String>,
    values: BTreeMap<String, String>,
}

impl DesktopEntry {
    pub fn parse_file(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref();
        Ok(Self::parse(path, &fs::read_to_string(path)?))
    }

    pub fn parse(path: impl Into<PathBuf>, source: &str) -> Self {
        let mut values = BTreeMap::new();
        let mut in_entry = false;
        for raw in source.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                in_entry = line == "[Desktop Entry]";
                continue;
            }
            if in_entry {
                if let Some((key, value)) = line.split_once('=') {
                    values.insert(key.trim().to_owned(), unescape(value.trim()));
                }
            }
        }
        Self {
            path: path.into(),
            id: None,
            values,
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }
    pub fn value(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }
    pub fn entry_type(&self) -> Option<&str> {
        self.value("Type")
    }
    pub fn icon(&self) -> Option<&str> {
        self.value("Icon")
    }
    pub fn exec(&self) -> Option<&str> {
        self.value("Exec")
    }
    pub fn terminal(&self) -> bool {
        bool_value(self.value("Terminal"))
    }
    pub fn hidden(&self) -> bool {
        bool_value(self.value("Hidden"))
    }
    pub fn no_display(&self) -> bool {
        bool_value(self.value("NoDisplay"))
    }
    pub fn is_application(&self) -> bool {
        self.entry_type() == Some("Application")
    }
    pub fn is_visible_application(&self) -> bool {
        self.is_application() && !self.hidden() && !self.no_display() && self.exec().is_some()
    }

    /// Returns the localized name, preferring an exact locale and then its language.
    pub fn name(&self, locale: Option<&str>) -> Option<&str> {
        localized(&self.values, "Name", locale).or_else(|| self.value("Name"))
    }
    pub fn comment(&self, locale: Option<&str>) -> Option<&str> {
        localized(&self.values, "Comment", locale).or_else(|| self.value("Comment"))
    }
    pub fn categories(&self) -> Vec<&str> {
        list(self.value("Categories"))
    }
    pub fn mime_types(&self) -> Vec<&str> {
        list(self.value("MimeType"))
    }

    /// Produces argv for opening `files`, expanding desktop-entry field codes.
    /// A file argument is appended when `Exec` contains no file/URI field code,
    /// matching the interoperable behaviour expected by file managers.
    pub fn command_for_files(&self, files: &[PathBuf]) -> Option<Vec<String>> {
        let exec = self.exec()?;
        let mut used_file_code = false;
        let mut output = Vec::new();
        for argument in split_exec(exec) {
            let expanded = expand_argument(&argument, self, files, &mut used_file_code);
            output.extend(expanded);
        }
        if !used_file_code {
            output.extend(files.iter().map(|file| file.to_string_lossy().into_owned()));
        }
        (!output.is_empty()).then_some(output)
    }

    pub fn program(&self) -> Option<String> {
        self.command_for_files(&[])
            .and_then(|argv| argv.into_iter().next())
    }
}

/// Finds installed desktop entries, respecting data-directory precedence and
/// hidden per-user overrides. `id` is the freedesktop desktop-file ID.
pub fn installed_applications(paths: &XdgPaths) -> Vec<DesktopEntry> {
    let mut entries = BTreeMap::new();
    for directory in paths.application_dirs() {
        scan(&directory, "", &mut entries);
    }
    entries.into_values().collect()
}

pub fn find_application(paths: &XdgPaths, id: &str) -> Option<DesktopEntry> {
    installed_applications(paths)
        .into_iter()
        .find(|entry| entry.id() == Some(id))
}

fn scan(directory: &Path, prefix: &str, entries: &mut BTreeMap<String, DesktopEntry>) {
    let Ok(read) = fs::read_dir(directory) else {
        return;
    };
    for item in read.flatten() {
        let path = item.path();
        let name = item.file_name().to_string_lossy().into_owned();
        if item.file_type().is_ok_and(|kind| kind.is_dir()) {
            scan(&path, &format!("{prefix}{name}-"), entries);
        } else if name.ends_with(".desktop") {
            let id = format!("{prefix}{name}");
            entries.entry(id.clone()).or_insert_with(|| {
                DesktopEntry::parse_file(&path)
                    .unwrap_or_else(|_| DesktopEntry::parse(&path, ""))
                    .with_id(id)
            });
        }
    }
}

fn bool_value(value: Option<&str>) -> bool {
    value.is_some_and(|value| value.eq_ignore_ascii_case("true"))
}
fn list(value: Option<&str>) -> Vec<&str> {
    value
        .into_iter()
        .flat_map(|value| value.split(';'))
        .filter(|value| !value.is_empty())
        .collect()
}
fn localized<'a>(
    values: &'a BTreeMap<String, String>,
    key: &str,
    locale: Option<&str>,
) -> Option<&'a str> {
    let locale = locale?;
    let locale = locale.split('.').next().unwrap_or(locale);
    values
        .get(&format!("{key}[{locale}]"))
        .or_else(|| {
            locale
                .split_once('_')
                .and_then(|(language, _)| values.get(&format!("{key}[{language}]")))
        })
        .map(String::as_str)
}
fn unescape(value: &str) -> String {
    let mut out = String::new();
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            out.push(match character {
                's' => ' ',
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                other => other,
            });
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            out.push(character);
        }
    }
    if escaped {
        out.push('\\');
    }
    out
}
fn split_exec(value: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut quote = false;
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            current.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' {
            quote = !quote;
        } else if character.is_whitespace() && !quote {
            if !current.is_empty() {
                result.push(std::mem::take(&mut current));
            }
        } else {
            current.push(character);
        }
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}
fn expand_argument(
    argument: &str,
    entry: &DesktopEntry,
    files: &[PathBuf],
    used_file_code: &mut bool,
) -> Vec<String> {
    if argument == "%F" {
        *used_file_code = true;
        return files
            .iter()
            .map(|file| file.to_string_lossy().into_owned())
            .collect();
    }
    if argument == "%U" {
        *used_file_code = true;
        return files.iter().map(file_uri).collect();
    }
    if argument == "%i" {
        return entry
            .icon()
            .map(|icon| vec!["--icon".into(), icon.into()])
            .unwrap_or_default();
    }
    let mut result = String::new();
    let mut chars = argument.chars();
    while let Some(character) = chars.next() {
        if character != '%' {
            result.push(character);
            continue;
        }
        match chars.next() {
            Some('%') => result.push('%'),
            Some('f') => {
                *used_file_code = true;
                if let Some(file) = files.first() {
                    result.push_str(&file.to_string_lossy());
                }
            }
            Some('u') => {
                *used_file_code = true;
                if let Some(file) = files.first() {
                    result.push_str(&file_uri(file));
                }
            }
            Some('c') => result.push_str(entry.name(None).unwrap_or_default()),
            Some('k') => result.push_str(&entry.path.to_string_lossy()),
            Some(_) | None => {}
        }
    }
    (!result.is_empty()).then_some(result).into_iter().collect()
}
fn file_uri(path: &PathBuf) -> String {
    let mut value = String::from("file://");
    for byte in path.as_os_str().as_encoded_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'/' | b'.' | b'-' | b'_' | b'~') {
            value.push(*byte as char);
        } else {
            value.push_str(&format!("%{byte:02X}"));
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expands_exec_codes_without_a_shell() {
        let entry = DesktopEntry::parse("editor.desktop", "[Desktop Entry]\nType=Application\nName=Editor\nIcon=editor\nExec=editor --name %c %F %%\n");
        assert_eq!(
            entry
                .command_for_files(&[PathBuf::from("a b.txt")])
                .unwrap(),
            ["editor", "--name", "Editor", "a b.txt", "%"]
        );
    }
    #[test]
    fn locale_falls_back_to_language() {
        let entry = DesktopEntry::parse(
            "x.desktop",
            "[Desktop Entry]\nName=English\nName[es]=Español\n",
        );
        assert_eq!(entry.name(Some("es_AR.UTF-8")), Some("Español"));
    }
}
