# delib

`delib` is a small, toolkit-agnostic Rust library for Linux desktop integration.
It provides standards-compliant XDG paths, desktop-entry and MIME-app lookup,
icon themes, and sound themes without coupling an application to a GUI toolkit
or compositor.

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

## Development

```bash
cargo fmt --check
cargo test
```

## License

Licensed under Apache-2.0.
