# delib

`delib` is a small, toolkit-agnostic Rust library for desktop integration.
It provides standards-compliant XDG paths, desktop-entry and MIME-app lookup,
icon themes, sound themes, and native Windows shell integration without
coupling an application to a GUI toolkit or compositor.

The default `xdg` feature provides the Freedesktop modules. On Windows, the
opt-in `windows` feature provides file associations, native RGBA shell icons,
and opening files, executables, directories, and shortcuts with their default
shell verb. A Windows-only consumer can use `default-features = false` with
`features = ["windows"]` to omit all XDG modules.

## Desktop entries and MIME applications

`desktop_entry` reads `.desktop` files, including localized fields and safe
`Exec` expansion (it never invokes a shell). `mimeapps` implements the XDG
MIME Apps search order, including desktop-specific `mimeapps.list` files.

```rust
use delib::{mimeapps::MimeApps, xdg::XdgPaths};

let handler = MimeApps::new(XdgPaths::default())
    .default_application("application/pdf");
```

## Sound themes

`sounds` resolves named events using the Sound Theme Specification. It follows
theme inheritance and `freedesktop`, understands profiles such as `stereo`,
and reports `.disabled` masks distinctly from missing events.

## Icon themes

The `icons` module resolves semantic names such as `folder`, `user-home`, or
`application-pdf` using the Freedesktop Icon Theme Specification.

- Finds themes in `~/.icons`, `$XDG_DATA_HOME/icons`, and
  `$XDG_DATA_DIRS/icons`.
- Reads `index.theme`, including size, HiDPI scale, and inheritance metadata.
- Falls back through parent themes, `hicolor`, then unthemed pixmaps.
- Detects the selected system pack from KDE/Plasma (`kdeglobals`) with GTK
  settings as a portable fallback.

The crate resolves file paths only. Decode SVG, PNG, or another supported
format in the GUI, image, or rendering library used by your application.

```rust
use delib::icons::{system_icon_theme, IconRequest, IconResolver};

let icons = IconResolver::new(system_icon_theme());
let folder = icons.resolve(IconRequest {
    names: vec!["folder-documents".into(), "folder".into()],
    size: 32,
    scale: 1,
});
```

An application may offer an explicit development override without inventing a
desktop-wide environment variable:

```rust
use delib::icons::system_icon_theme_with_override;

let theme = system_icon_theme_with_override(Some("MY_APP_ICON_THEME"));
```

## Windows shell integration

```rust
#[cfg(windows)]
{
    let association = delib::windows::file_association("document.pdf")?;
    let icon = delib::windows::file_icon("Shortcut.lnk", 48)?;
    delib::windows::open_path("Shortcut.lnk")?;
}
```

`file_icon` returns straight RGBA pixels and dimensions. `open_path` uses the
default shell verb and never constructs a command line from an association.

## Development

```bash
cargo fmt --check
cargo test
cargo test --no-default-features --features windows # Windows
```

## License

Licensed under Apache-2.0.
