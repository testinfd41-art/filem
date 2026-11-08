# Filem

A quiet, keyboard-first file manager for Linux, built for NixOS, niri and
Noctalia. The UI is Qt Quick; the filesystem and controller logic are Rust in a
single process. No daemon, database, IPC socket, or second GUI toolkit.

The project is an in-progress implementation of the supplied development plan.
The browser foundation is working, and selected later-phase functionality has
also been added. This is **not** yet a complete implementation of the plan;
the status below distinguishes connected features from unfinished modules.

## What works now

- Asynchronous directory loading, natural sorting, list/grid views, selection,
  tabs, navigation history, and optional session restoration
- Breadcrumb navigation, editable location bar, XDG places, persistent
  bookmarks, Trash, and live directory refresh
- Copy/move, batch rename, create, trash/restore, conflict handling,
  progress/cancellation, and supported-operation undo/redo
- Current-folder search with optional recursion, ranked matches, and filters
  for tags, extensions, types, sizes, and dates
- Persistent seven-color file tags, including context-menu assignment and
  `T` followed by `1`–`7` keyboard shortcuts
- Tag assignments and custom tag labels are stored in
  `$XDG_STATE_HOME/filem/tags.json` (or `~/.local/state/filem/tags.json`);
  assignments use device/inode identity with an absolute-path fallback
- Quick Look for images, text, archives, first-page PDF previews, and video
  playback where the installed Qt multimedia backend supports it
- Image conversion to every writable format supported by ImageMagick on the
  system, including HEIC when its delegate is available; originals are kept
- Desktop notifications for completed, failed, or canceled background file
  operations
- ZIP, TAR, and TAR.GZ creation; ZIP/TAR extraction and TAR.GZ, TAR.BZ2,
  TAR.XZ, and TAR.ZST extraction when the corresponding decoders are installed
- Archive extraction constrained to a chosen folder, with traversal and
  symlink protections, no overwrite, entry limits, and expanded-size limits
- Open With app picker, metadata integrated into the preview pane, Extract
  Here/To, and terminal action
- Removable-device discovery and user-confirmed mount, unmount, unlock, and
  eject actions; GVfs-backed MTP devices and network locations where available
- Live theme reload through `~/.config/filem/theme.json`, Noctalia palette
  integration (read-only), built-in dark/light palettes, and persistent UI
  preferences
- Internal and local-file drag/drop, hidden-file controls, and clear operation
  errors

The project is still incomplete. Column mode is a basic breadcrumb-based view,
not a full adjacent-column browser. There is no dual-pane UI, tag-name settings
screen, or safety-critical disk-image writer. Device and network
support depends on the system's GVfs/UDisks backends. PDF rendering and video
playback depend on available system/Qt components. Permanent deletion cannot
be undone; other undo support is limited to operations with retained recovery
data.

Tags are application metadata, not written into the files. A file copied outside
Filem does not inherit its tag; removable and network filesystems may not keep
stable inode identities. The preview pane combines content previews with filesystem metadata including
type, size, timestamps, permissions, numeric owner/group IDs, device and inode
IDs, link target, and the current Filem tag. The Properties context-menu action
opens this same pane when needed.

## Requirements

- NixOS 26.05 (or a nixpkgs 26.05-compatible system)
- Wayland with niri recommended; any Qt-supported desktop should work
- `xdg-open` at runtime (already present on this system)
- ImageMagick at runtime for image conversion (provided by the flake)
- A desktop notification service for operation notifications
- OpenGL/Wayland libraries come from the Qt packages in the dev shell

You do **not** need to install Rust, Qt, `pkg-config`, or compiler tools
system-wide. The flake provides them. No additional NixOS packages are required
for Phase 1.

Without entering the dev shell, the installed package still needs a working Qt
runtime and an SVG image plugin for file icons. The flake package wraps itself
with the correct plugin, QML and library paths.

## Development

```bash
cd /home/chris/dEV/filem
nix develop
cargo test
cargo run
```

`cargo run` with no argument opens your home directory. You can also start
somewhere specific:

```bash
cargo run -- /var/log
```

Headless smoke test (loads the full QML tree, then exits):

```bash
nix develop --command env \
  QT_QPA_PLATFORM=offscreen \
  QT_QUICK_BACKEND=software \
  cargo run -- --smoke-test
```

Inspect the resolved theme without opening a window:

```bash
nix develop --command cargo run -- --print-theme
```

Build and run the packaged application:

```bash
nix build .#filem
./result/bin/filem
```

## Keyboard

| Keys | Action |
| --- | --- |
| `↑` / `↓`, `PageUp` / `PageDown`, `Home` / `End` | Move through files |
| `Enter` | Open folder or file |
| `Backspace` | Go to parent folder |
| `Alt+Left` / `Alt+Right` | Back / forward |
| `Alt+Up` | Parent folder |
| `Ctrl+L` | Edit location |
| `Escape` in the location bar | Cancel editing |
| `Ctrl+H` | Show/hide hidden files |
| `Ctrl+R` or `F5` | Refresh |
| `Ctrl+B` | Toggle sidebar |
| `T`, then `1`–`7` | Assign a tag to the selection |
| `Alt+1` / `Alt+2` | Focus sidebar / file list |
| `Ctrl+Shift+D` | Toggle built-in dark/light theme |
| `Ctrl+Q` | Quit |

Click a column header to sort by it; click the active header again to reverse.
The toolbar has buttons for back, forward, parent, refresh, hidden files,
folders-first and the sidebar.

## Configuration

Preferences are stored at `~/.config/filem/config.toml` and saved atomically
when changed from the UI. The file is ordinary TOML and can be edited while the
app is closed:

```toml
[ui]
show_hidden = false
sort_key = "name"
sort_ascending = true
directories_first = true
single_click_open = false
sidebar_visible = true
```

Invalid or partial config files fall back to safe defaults; an invalid file is
reported in the app's error bar rather than replacing the file.

## Theming

Theme resolution order:

1. `~/.config/filem/theme.json` (normally written by a Noctalia user template)
2. A readable Noctalia community or custom palette matching the current mode
3. Built-in dark/light palettes

The theme file is watched and reloaded live. Invalid colors are ignored, and
opacity is clamped to a readable range. See `themes/dark.json` and
`themes/light.json` for complete examples.

Open **More actions → Appearance…** to change the installed UI font (or enter a
custom family name), font size, and interface or tag colors. These Filem-owned
choices are saved in
`~/.config/filem/appearance.json`; they overlay the theme and never modify
Noctalia configuration or its generated `theme.json`. Window blur must be
configured as a rule in the desktop compositor; there is no portable in-app
blur setting.

When an external theme file or Noctalia settings are newer than a saved Filem
mode, the external mode takes precedence and Filem removes only that outdated
mode override. Other appearance settings are preserved. A later manual mode
toggle remains active until the next external theme update.

### Optional Noctalia integration

Filem never edits Noctalia's configuration. To let Noctalia render Filem's theme
automatically, copy the supplied template and add a user template entry.

Exact commands:

```bash
mkdir -p ~/.config/noctalia/templates
cp /home/chris/dEV/filem/themes/noctalia.tmpl ~/.config/noctalia/templates/filem.tmpl
```

Then add this to `~/.config/noctalia/templates.toml`:

```toml
[theme.templates.user.filem]
input_path = "$XDG_CONFIG_HOME/noctalia/templates/filem.tmpl"
output_path = "$XDG_CONFIG_HOME/filem/theme.json"
```

Noctalia will rewrite `~/.config/filem/theme.json` whenever the palette changes;
Filem picks it up without a restart. The template can be tested without
changing any settings:

```bash
noctalia theme ~/walls/IMG_20261102_221216_416.png --default-mode light \
  -r /home/chris/dEV/filem/themes/noctalia.tmpl:/tmp/filem-theme.json
```

Adjust the image path to your current wallpaper. The output is valid JSON and
can be checked with `jq empty /tmp/filem-theme.json`.

## Project layout

```text
src/main.rs       app startup, CLI, QML registration
src/backend.rs    QML controller and QAbstractListModel, async loading, history
src/fs_entry.rs   directory reading, metadata, sorting
src/natsort.rs    natural Unicode-aware comparison
src/paths.rs      XDG paths, user places, breadcrumbs, file URLs
src/icons.rs      freedesktop icon lookup and file-kind mapping
src/config.rs     TOML preferences and atomic persistence
src/theme.rs      theme model, Noctalia mapping, live file watching
qml/Main.qml      window, shortcuts, layout
qml/components/   toolbar, sidebar, breadcrumbs, list, status bar
themes/           built-in theme examples and the Noctalia template
flake.nix         reproducible Rust + Qt development shell and package
```

## Design notes

- One process, one window, worker threads only for filesystem reads and theme
  watching. Results return to the Qt thread through queued callbacks.
- Generation counters prevent a slow read from replacing a newer directory.
- Directory reads use `symlink_metadata` first, then follow links, so broken
  links are displayed instead of aborting the whole listing.
- Paths are cleaned lexically and symlinked directories keep the path the user
  typed; resolving links is a later, explicit feature.
- Errors stay in the file manager. A failed navigation does not clear the
  current folder.
- Icons are resolved to local `file://` URLs, avoiding a dependency on Qt's
  icon provider or a specific desktop theme.

## Not implemented yet

Dual-pane browsing, a full adjacent-column browser, tag customization UI, and
the safety-critical disk-image writer remain unfinished. The disk writer is
deliberately deferred until normal file-management workflows are stable and
can be tested against a safe disposable target.
