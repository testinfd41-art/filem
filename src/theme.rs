//! Runtime theme model and live theme-file watching.
//!
//! Priority:
//! 1. `~/.config/filem/theme.json` (normally rendered by a Noctalia user template)
//! 2. a resolved Noctalia community/custom palette, when one is readable
//! 3. built-in dark/light palettes
//!
//! Noctalia's own configuration is never written here.

use std::collections::HashMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use qmetaobject::prelude::*;
use serde::Deserialize;

use crate::paths::{app_config_dir, appearance_file, config_dir, state_dir, theme_file};

const NOCTALIA_NAME_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'%')
    .add(b'/')
    .add(b'\\')
    .add(b'?')
    .add(b'#');

#[derive(Debug, Clone)]
pub struct ThemeData {
    pub mode: String,
    pub background: String,
    pub surface: String,
    pub surface_alt: String,
    pub foreground: String,
    pub muted: String,
    pub accent: String,
    pub border: String,
    pub selection: String,
    pub selection_text: String,
    pub hover: String,
    pub focus: String,
    pub danger: String,
    pub tags: [String; 7],
    pub radius: f64,
    pub spacing: f64,
    pub row_height: f64,
    pub opacity: f64,
    pub font_size: f64,
    pub font_family: String,
    pub mono_font: String,
}

#[derive(Debug, Clone)]
pub struct LoadedTheme {
    pub data: ThemeData,
    pub warning: Option<String>,
    pub source: String,
}

impl ThemeData {
    pub fn builtin_dark() -> Self {
        Self {
            mode: "dark".to_owned(),
            background: "#0f1115".to_owned(),
            surface: "#151820".to_owned(),
            surface_alt: "#1c202a".to_owned(),
            foreground: "#e7e9ee".to_owned(),
            muted: "#9299a8".to_owned(),
            accent: "#8ab4f8".to_owned(),
            border: "#292e3a".to_owned(),
            selection: "#24364f".to_owned(),
            selection_text: "#f2f5fb".to_owned(),
            hover: "#202531".to_owned(),
            focus: "#8ab4f8".to_owned(),
            danger: "#f28b82".to_owned(),
            tags: [
                "#f28b82".to_owned(),
                "#f5a97f".to_owned(),
                "#e5c07b".to_owned(),
                "#8fbf7f".to_owned(),
                "#8ab4f8".to_owned(),
                "#c39bea".to_owned(),
                "#9aa0ac".to_owned(),
            ],
            radius: 10.0,
            spacing: 8.0,
            row_height: 34.0,
            opacity: 1.0,
            font_size: 12.0,
            font_family: "Ubuntu Sans".to_owned(),
            mono_font: "JetBrainsMono Nerd Font".to_owned(),
        }
    }

    pub fn builtin_light() -> Self {
        Self {
            mode: "light".to_owned(),
            background: "#f6f7f9".to_owned(),
            surface: "#ffffff".to_owned(),
            surface_alt: "#eef0f4".to_owned(),
            foreground: "#1c1f26".to_owned(),
            muted: "#5f6672".to_owned(),
            accent: "#2f6fd0".to_owned(),
            border: "#d8dce4".to_owned(),
            selection: "#d8e5fb".to_owned(),
            selection_text: "#13213a".to_owned(),
            hover: "#e9edf4".to_owned(),
            focus: "#2f6fd0".to_owned(),
            danger: "#b3261e".to_owned(),
            tags: [
                "#c5221f".to_owned(),
                "#c26401".to_owned(),
                "#9a6a00".to_owned(),
                "#2e7d32".to_owned(),
                "#1a5fb4".to_owned(),
                "#7b3fa0".to_owned(),
                "#5f6672".to_owned(),
            ],
            radius: 10.0,
            spacing: 8.0,
            row_height: 34.0,
            opacity: 1.0,
            font_size: 12.0,
            font_family: "Ubuntu Sans".to_owned(),
            mono_font: "JetBrainsMono Nerd Font".to_owned(),
        }
    }

    pub fn builtin_for_mode(mode: &str) -> Self {
        if mode == "light" {
            Self::builtin_light()
        } else {
            Self::builtin_dark()
        }
    }

    pub fn load() -> LoadedTheme {
        let appearance_path = appearance_file();
        let appearance = std::fs::read_to_string(&appearance_path)
            .ok()
            .and_then(|text| ThemeFile::parse(&text).ok());
        if let Some(appearance) = appearance {
            return Self::apply_appearance(Self::load_base(), appearance);
        }
        Self::load_base()
    }

    fn load_manual_mode(mode: &str) -> LoadedTheme {
        let appearance = std::fs::read_to_string(appearance_file())
            .ok()
            .and_then(|text| ThemeFile::parse(&text).ok());
        let mut data = Self::builtin_for_mode(mode);
        if let Some(mut appearance) = appearance {
            appearance.mode = None;
            data = data.apply_file(appearance);
        }
        LoadedTheme {
            data,
            warning: None,
            source: "manual".to_owned(),
        }
    }

    fn apply_appearance(mut loaded: LoadedTheme, appearance: ThemeFile) -> LoadedTheme {
        if let Some(mode) = appearance.mode.as_deref() {
            if mode == "light" || mode == "dark" {
                loaded.data = ThemeData::builtin_for_mode(mode);
            }
        }
        loaded.data = loaded.data.apply_file(appearance);
        loaded.source = "Filem appearance".to_owned();
        loaded
    }

    fn load_base() -> LoadedTheme {
        let theme_path = theme_file();
        if theme_path.exists() {
            return match std::fs::read_to_string(&theme_path) {
                Ok(text) => match ThemeFile::parse(&text) {
                    Ok(file) => {
                        let base =
                            ThemeData::builtin_for_mode(file.mode.as_deref().unwrap_or("dark"));
                        LoadedTheme {
                            data: base.apply_file(file),
                            warning: None,
                            source: "theme file".to_owned(),
                        }
                    }
                    Err(error) => LoadedTheme {
                        data: ThemeData::builtin_dark(),
                        warning: Some(format!(
                            "Theme file {} is invalid: {error}. Using the built-in dark theme.",
                            theme_path.display()
                        )),
                        source: "built-in".to_owned(),
                    },
                },
                Err(error) => LoadedTheme {
                    data: ThemeData::builtin_dark(),
                    warning: Some(format!(
                        "Could not read {}: {error}. Using the built-in dark theme.",
                        theme_path.display()
                    )),
                    source: "built-in".to_owned(),
                },
            };
        }

        let noctalia = read_noctalia_settings();
        let mode = noctalia
            .as_ref()
            .and_then(|settings| settings.theme.mode.clone())
            .filter(|mode| mode == "light" || mode == "dark")
            .unwrap_or_else(|| "dark".to_owned());
        let base = ThemeData::builtin_for_mode(&mode);

        if let Some(palette) = noctalia
            .as_ref()
            .and_then(|settings| resolve_noctalia_palette(settings, &mode))
        {
            return LoadedTheme {
                data: base.apply_noctalia(palette),
                warning: None,
                source: "Noctalia".to_owned(),
            };
        }

        LoadedTheme {
            data: base,
            warning: None,
            source: "built-in".to_owned(),
        }
    }

    fn apply_file(&self, file: ThemeFile) -> Self {
        let mut out = self.clone();
        if let Some(mode) = file.mode {
            if mode == "light" || mode == "dark" {
                out.mode = mode;
            }
        }
        macro_rules! set_color {
            ($field:ident, $value:expr) => {
                if let Some(value) = $value {
                    if valid_color(&value) {
                        out.$field = value;
                    }
                }
            };
        }

        set_color!(background, file.background);
        set_color!(surface, file.surface);
        set_color!(surface_alt, file.surface_alt);
        set_color!(foreground, file.foreground);
        set_color!(muted, file.muted);
        set_color!(accent, file.accent);
        set_color!(border, file.border);
        set_color!(selection, file.selection);
        set_color!(selection_text, file.selection_text);
        set_color!(hover, file.hover);
        set_color!(focus, file.focus);
        set_color!(danger, file.danger);

        if let Some(tags) = file.tags {
            for (index, value) in tags.into_iter().take(7).enumerate() {
                if valid_color(&value) {
                    out.tags[index] = value;
                }
            }
        }
        if let Some(radius) = file.radius {
            out.radius = radius.clamp(0.0, 24.0);
        }
        if let Some(spacing) = file.spacing {
            out.spacing = spacing.clamp(0.0, 24.0);
        }
        if let Some(row_height) = file.row_height {
            out.row_height = row_height.clamp(24.0, 64.0);
        }
        if let Some(opacity) = file.opacity {
            // Never let a theme make the interface unreadably transparent.
            out.opacity = opacity.clamp(0.55, 1.0);
        }
        if let Some(font_size) = file.font_size {
            out.font_size = font_size.clamp(9.0, 24.0);
        }
        if let Some(font) = file.font_family {
            if !font.trim().is_empty() && font.len() < 120 {
                out.font_family = font;
            }
        }
        if let Some(font) = file.mono_font {
            if !font.trim().is_empty() && font.len() < 120 {
                out.mono_font = font;
            }
        }
        out
    }

    fn apply_noctalia(&self, palette: NoctaliaVariant) -> Self {
        let mut out = self.clone();
        macro_rules! set_color {
            ($field:ident, $value:expr) => {
                if let Some(value) = $value {
                    if valid_color(&value) {
                        out.$field = value;
                    }
                }
            };
        }

        set_color!(background, palette.surface.clone());
        set_color!(
            surface,
            palette
                .surface_container
                .clone()
                .or(palette.surface.clone())
        );
        set_color!(surface_alt, palette.surface_variant.clone());
        set_color!(foreground, palette.on_surface.clone());
        set_color!(muted, palette.on_surface_variant.clone());
        set_color!(accent, palette.primary.clone());
        set_color!(border, palette.outline.clone());
        set_color!(selection, palette.hover.clone());
        set_color!(selection_text, palette.on_hover.clone());
        set_color!(hover, palette.surface_variant.clone());
        set_color!(focus, palette.primary.clone());
        set_color!(danger, palette.error.clone());

        if let Some(terminal) = palette.terminal {
            let normal = terminal.normal.unwrap_or_default();
            let bright = terminal.bright.unwrap_or_default();
            let tag_sources = [
                normal.get("red"),
                bright.get("yellow").or_else(|| normal.get("yellow")),
                normal.get("yellow"),
                normal.get("green"),
                normal.get("blue"),
                normal.get("magenta"),
                normal.get("white"),
            ];
            for (index, value) in tag_sources.into_iter().enumerate() {
                if let Some(value) = value {
                    if valid_color(value) {
                        out.tags[index] = value.clone();
                    }
                }
            }
        }
        out
    }
}

fn valid_color(value: &str) -> bool {
    let value = value.trim();
    if value.len() != 7 || !value.starts_with('#') {
        return false;
    }
    value[1..].chars().all(|c| c.is_ascii_hexdigit())
}

fn validated_appearance_value(key: &str, value: &str) -> Option<serde_json::Value> {
    let value = value.trim();
    match key {
        "mode" if value == "dark" || value == "light" => {
            Some(serde_json::Value::String(value.to_owned()))
        }
        "opacity" => value
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
            .map(|number| serde_json::json!(number.clamp(0.55, 1.0))),
        "fontSize" => value
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
            .map(|number| serde_json::json!(number.clamp(9.0, 24.0))),
        "fontFamily" if !value.is_empty() && value.len() < 120 => {
            Some(serde_json::Value::String(value.to_owned()))
        }
        "background" | "surface" | "surfaceAlt" | "foreground" | "muted" | "accent" | "border"
        | "selection" | "selectionText" | "hover" | "focus" | "danger"
            if valid_color(value) =>
        {
            Some(serde_json::Value::String(value.to_ascii_lowercase()))
        }
        _ if key.starts_with("tag") => {
            let index = key.strip_prefix("tag")?.parse::<usize>().ok()?;
            if !(1..=7).contains(&index) || !valid_color(value) {
                return None;
            }
            Some(serde_json::Value::String(value.to_ascii_lowercase()))
        }
        _ => None,
    }
}

fn write_appearance_value(
    path: &Path,
    key: &str,
    value: serde_json::Value,
    fallback_tags: &[String; 7],
) -> Result<(), String> {
    if std::fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err("appearance settings file must not be a symlink".to_owned());
    }
    let mut document = if path.exists() {
        let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
        serde_json::from_str::<serde_json::Value>(&text)
            .map_err(|error| format!("invalid appearance settings: {error}"))?
    } else {
        serde_json::json!({})
    };
    let object = document
        .as_object_mut()
        .ok_or_else(|| "appearance settings must be a JSON object".to_owned())?;
    if let Some(tag) = key.strip_prefix("tag") {
        let index = tag
            .parse::<usize>()
            .ok()
            .filter(|index| (1..=7).contains(index))
            .ok_or_else(|| "invalid tag color key".to_owned())?;
        let tags = object.entry("tags").or_insert_with(|| {
            serde_json::Value::Array(
                fallback_tags
                    .iter()
                    .cloned()
                    .map(serde_json::Value::String)
                    .collect(),
            )
        });
        if !tags.is_array() {
            *tags = serde_json::Value::Array(
                fallback_tags
                    .iter()
                    .cloned()
                    .map(serde_json::Value::String)
                    .collect(),
            );
        }
        let tags = tags.as_array_mut().expect("tags was set to an array");
        while tags.len() < 7 {
            let index = tags.len();
            tags.push(serde_json::Value::String(fallback_tags[index].clone()));
        }
        for (index, tag) in tags.iter_mut().enumerate().take(7) {
            if !tag.is_string() {
                *tag = serde_json::Value::String(fallback_tags[index].clone());
            }
        }
        tags[index - 1] = value;
    } else {
        object.insert(key.to_owned(), value);
    }

    let parent = path
        .parent()
        .ok_or_else(|| "appearance settings path has no parent".to_owned())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let bytes = serde_json::to_vec_pretty(&document).map_err(|error| error.to_string())?;
    let mut temp_path = None;
    let mut temp_file = None;
    for suffix in 0..100 {
        let candidate = parent.join(format!(
            ".appearance.json.{}.{}.tmp",
            std::process::id(),
            suffix
        ));
        let opened = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&candidate);
        match opened {
            Ok(file) => {
                temp_path = Some(candidate);
                temp_file = Some(file);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
    let temp_path = temp_path.ok_or_else(|| "could not create settings file".to_owned())?;
    let result = (|| {
        let mut file = temp_file.expect("temp path and file are created together");
        file.write_all(&bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        std::fs::rename(&temp_path, path).map_err(|error| error.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

pub fn is_valid_color(value: &str) -> bool {
    valid_color(value)
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct ThemeFile {
    mode: Option<String>,
    background: Option<String>,
    surface: Option<String>,
    surface_alt: Option<String>,
    #[serde(rename = "surfaceAlt")]
    surface_alt_camel: Option<String>,
    foreground: Option<String>,
    muted: Option<String>,
    accent: Option<String>,
    border: Option<String>,
    selection: Option<String>,
    selection_text: Option<String>,
    #[serde(rename = "selectionText")]
    selection_text_camel: Option<String>,
    hover: Option<String>,
    focus: Option<String>,
    danger: Option<String>,
    tags: Option<Vec<String>>,
    radius: Option<f64>,
    spacing: Option<f64>,
    row_height: Option<f64>,
    #[serde(rename = "rowHeight")]
    row_height_camel: Option<f64>,
    opacity: Option<f64>,
    #[serde(rename = "fontSize")]
    font_size: Option<f64>,
    font_family: Option<String>,
    #[serde(rename = "fontFamily")]
    font_family_camel: Option<String>,
    mono_font: Option<String>,
    #[serde(rename = "monoFont")]
    mono_font_camel: Option<String>,
}

impl ThemeFile {
    fn parse(text: &str) -> Result<Self, String> {
        let mut file: ThemeFile = serde_json::from_str(text).map_err(|error| error.to_string())?;
        // Merge camelCase aliases into the canonical slots.
        file.surface_alt = file.surface_alt_camel.take().or(file.surface_alt);
        file.selection_text = file.selection_text_camel.take().or(file.selection_text);
        file.row_height = file.row_height_camel.take().or(file.row_height);
        file.font_family = file.font_family_camel.take().or(file.font_family);
        file.mono_font = file.mono_font_camel.take().or(file.mono_font);
        Ok(file)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct NoctaliaSettings {
    theme: NoctaliaThemeConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct NoctaliaThemeConfig {
    source: Option<String>,
    mode: Option<String>,
    community_palette: Option<String>,
    custom_palette: Option<String>,
    builtin: Option<String>,
}

fn read_noctalia_settings() -> Option<NoctaliaSettings> {
    let candidates = [
        state_dir().join("noctalia/settings.toml"),
        config_dir().join("noctalia/config.toml"),
    ];
    for path in candidates {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(settings) = toml::from_str::<NoctaliaSettings>(&text) {
                return Some(settings);
            }
        }
    }
    None
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct NoctaliaPalette {
    dark: Option<NoctaliaVariant>,
    light: Option<NoctaliaVariant>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct NoctaliaVariant {
    #[serde(rename = "mPrimary")]
    primary: Option<String>,
    #[serde(rename = "mOnPrimary")]
    _on_primary: Option<String>,
    #[serde(rename = "mSurface")]
    surface: Option<String>,
    #[serde(rename = "mOnSurface")]
    on_surface: Option<String>,
    #[serde(rename = "mSurfaceVariant")]
    surface_variant: Option<String>,
    #[serde(rename = "mOnSurfaceVariant")]
    on_surface_variant: Option<String>,
    #[serde(rename = "mOutline")]
    outline: Option<String>,
    #[serde(rename = "mHover")]
    hover: Option<String>,
    #[serde(rename = "mOnHover")]
    on_hover: Option<String>,
    #[serde(rename = "mError")]
    error: Option<String>,
    #[serde(rename = "mSurfaceContainer")]
    surface_container: Option<String>,
    terminal: Option<NoctaliaTerminal>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct NoctaliaTerminal {
    normal: Option<HashMap<String, String>>,
    bright: Option<HashMap<String, String>>,
}

fn resolve_noctalia_palette(settings: &NoctaliaSettings, mode: &str) -> Option<NoctaliaVariant> {
    let source = settings.theme.source.as_deref()?;
    let palette_path = match source {
        "community" => {
            let name = settings.theme.community_palette.as_deref()?;
            let encoded = utf8_percent_encode(name, NOCTALIA_NAME_SET).to_string();
            Some(
                state_dir()
                    .join("noctalia/community-palettes")
                    .join(format!("{encoded}.json")),
            )
        }
        "custom" => {
            let name = settings.theme.custom_palette.as_deref()?;
            Some(
                config_dir()
                    .join("noctalia/palettes")
                    .join(format!("{name}.json")),
            )
        }
        _ => None,
    }?;

    let text = std::fs::read_to_string(palette_path).ok()?;
    let palette: NoctaliaPalette = serde_json::from_str(&text).ok()?;
    if mode == "light" {
        palette.light.or(palette.dark)
    } else {
        palette.dark.or(palette.light)
    }
}

#[derive(QObject)]
pub struct Theme {
    base: qt_base_class!(trait QObject),
    mode: qt_property!(QString; NOTIFY changed),
    background: qt_property!(QString; NOTIFY changed),
    surface: qt_property!(QString; NOTIFY changed),
    surface_alt: qt_property!(QString; NOTIFY changed),
    foreground: qt_property!(QString; NOTIFY changed),
    muted: qt_property!(QString; NOTIFY changed),
    accent: qt_property!(QString; NOTIFY changed),
    border: qt_property!(QString; NOTIFY changed),
    selection: qt_property!(QString; NOTIFY changed),
    selection_text: qt_property!(QString; NOTIFY changed),
    hover: qt_property!(QString; NOTIFY changed),
    focus: qt_property!(QString; NOTIFY changed),
    danger: qt_property!(QString; NOTIFY changed),
    tag1: qt_property!(QString; NOTIFY changed),
    tag2: qt_property!(QString; NOTIFY changed),
    tag3: qt_property!(QString; NOTIFY changed),
    tag4: qt_property!(QString; NOTIFY changed),
    tag5: qt_property!(QString; NOTIFY changed),
    tag6: qt_property!(QString; NOTIFY changed),
    tag7: qt_property!(QString; NOTIFY changed),
    radius: qt_property!(f64; NOTIFY changed),
    spacing: qt_property!(f64; NOTIFY changed),
    row_height: qt_property!(f64; NOTIFY changed),
    opacity: qt_property!(f64; NOTIFY changed),
    font_size: qt_property!(f64; NOTIFY changed),
    font_scale: qt_property!(f64; NOTIFY changed),
    font_family: qt_property!(QString; NOTIFY changed),
    mono_font: qt_property!(QString; NOTIFY changed),
    source: qt_property!(QString; NOTIFY changed),
    warning: qt_property!(QString; NOTIFY changed),
    changed: qt_signal!(),
    start: qt_method!(fn(&mut self)),
    reload: qt_method!(fn(&mut self)),
    toggle_mode: qt_method!(fn(&mut self)),
    tag_color: qt_method!(fn(&self, index: i32) -> QString),
    set_appearance: qt_method!(fn(&mut self, key: QString, value: QString) -> bool),

    data: ThemeData,
    manual_mode_override: Option<String>,
    watcher_started: bool,
}

impl Default for Theme {
    fn default() -> Self {
        let data = ThemeData::builtin_dark();
        Self {
            data,
            manual_mode_override: None,
            watcher_started: false,
            base: Default::default(),
            mode: Default::default(),
            background: Default::default(),
            surface: Default::default(),
            surface_alt: Default::default(),
            foreground: Default::default(),
            muted: Default::default(),
            accent: Default::default(),
            border: Default::default(),
            selection: Default::default(),
            selection_text: Default::default(),
            hover: Default::default(),
            focus: Default::default(),
            danger: Default::default(),
            tag1: Default::default(),
            tag2: Default::default(),
            tag3: Default::default(),
            tag4: Default::default(),
            tag5: Default::default(),
            tag6: Default::default(),
            tag7: Default::default(),
            radius: Default::default(),
            spacing: Default::default(),
            row_height: Default::default(),
            opacity: Default::default(),
            font_size: Default::default(),
            font_scale: Default::default(),
            font_family: Default::default(),
            mono_font: Default::default(),
            source: Default::default(),
            warning: Default::default(),
            changed: Default::default(),
            start: Default::default(),
            reload: Default::default(),
            toggle_mode: Default::default(),
            tag_color: Default::default(),
            set_appearance: Default::default(),
        }
    }
}

impl Theme {
    fn apply(&mut self, loaded: LoadedTheme) {
        self.data = loaded.data;
        self.mode = QString::from(self.data.mode.clone());
        self.background = QString::from(self.data.background.clone());
        self.surface = QString::from(self.data.surface.clone());
        self.surface_alt = QString::from(self.data.surface_alt.clone());
        self.foreground = QString::from(self.data.foreground.clone());
        self.muted = QString::from(self.data.muted.clone());
        self.accent = QString::from(self.data.accent.clone());
        self.border = QString::from(self.data.border.clone());
        self.selection = QString::from(self.data.selection.clone());
        self.selection_text = QString::from(self.data.selection_text.clone());
        self.hover = QString::from(self.data.hover.clone());
        self.focus = QString::from(self.data.focus.clone());
        self.danger = QString::from(self.data.danger.clone());
        self.tag1 = QString::from(self.data.tags[0].clone());
        self.tag2 = QString::from(self.data.tags[1].clone());
        self.tag3 = QString::from(self.data.tags[2].clone());
        self.tag4 = QString::from(self.data.tags[3].clone());
        self.tag5 = QString::from(self.data.tags[4].clone());
        self.tag6 = QString::from(self.data.tags[5].clone());
        self.tag7 = QString::from(self.data.tags[6].clone());
        self.radius = self.data.radius;
        self.spacing = self.data.spacing;
        self.row_height = self.data.row_height;
        self.opacity = self.data.opacity;
        self.font_size = self.data.font_size;
        self.font_scale = self.data.font_size / 12.0;
        self.font_family = QString::from(self.data.font_family.clone());
        self.mono_font = QString::from(self.data.mono_font.clone());
        self.source = QString::from(loaded.source);
        self.warning = QString::from(loaded.warning.unwrap_or_default());
        self.changed();
    }

    fn reload(&mut self) {
        if let Some(mode) = self.manual_mode_override.clone() {
            self.apply(ThemeData::load_manual_mode(&mode));
            return;
        }
        let loaded = ThemeData::load();
        self.apply(loaded);
    }

    fn toggle_mode(&mut self) {
        let current = self
            .manual_mode_override
            .clone()
            .unwrap_or_else(|| self.data.mode.clone());
        let target = if current == "light" {
            "dark".to_owned()
        } else {
            "light".to_owned()
        };
        self.manual_mode_override = Some(target.clone());
        self.apply(ThemeData::load_manual_mode(&target));
    }

    fn tag_color(&self, index: i32) -> QString {
        let index = index.clamp(0, 6) as usize;
        QString::from(self.data.tags[index].clone())
    }

    fn set_appearance(&mut self, key: QString, value: QString) -> bool {
        let key = key.to_string();
        let value = value.to_string();
        let Some(value) = validated_appearance_value(&key, &value) else {
            return false;
        };
        if write_appearance_value(&appearance_file(), &key, value, &self.data.tags).is_err() {
            return false;
        }
        self.manual_mode_override = None;
        self.reload();
        true
    }

    fn start(&mut self) {
        if self.watcher_started {
            return;
        }
        self.watcher_started = true;
        self.reload();

        let qptr = QPointer::from(&*self);
        let reload_callback = qmetaobject::queued_callback(move |()| {
            if let Some(theme) = qptr.as_pinned() {
                theme.borrow_mut().reload();
            }
        });

        std::thread::spawn(move || {
            let (tx, rx) = mpsc::channel::<()>();
            let mut watcher: RecommendedWatcher = match notify::recommended_watcher(
                move |result: Result<notify::Event, notify::Error>| {
                    if result.is_ok() {
                        let _ = tx.send(());
                    }
                },
            ) {
                Ok(watcher) => watcher,
                Err(_) => return,
            };

            let mut watched = Vec::<PathBuf>::new();
            let mut add = |path: PathBuf| {
                if path.is_dir()
                    && !watched.contains(&path)
                    && watcher.watch(&path, RecursiveMode::NonRecursive).is_ok()
                {
                    watched.push(path);
                }
            };

            let app_dir = app_config_dir();
            let _ = std::fs::create_dir_all(&app_dir);
            add(app_dir);
            add(state_dir().join("noctalia"));
            add(config_dir().join("noctalia"));
            add(config_dir().join("noctalia/palettes"));
            add(state_dir().join("noctalia/community-palettes"));

            while rx.recv().is_ok() {
                // Coalesce a burst of writes (editors commonly write temp files
                // and rename them) into one reload.
                std::thread::sleep(Duration::from_millis(140));
                while rx.try_recv().is_ok() {}
                reload_callback(());
            }
        });
    }
}

/// A convenience for tests and for the `--print-theme` developer command.
pub fn describe_theme(data: &ThemeData) -> String {
    format!(
        "mode={} source-path={} accent={} background={} foreground={}",
        data.mode,
        theme_file().display(),
        data.accent,
        data.background,
        data.foreground
    )
}

#[allow(dead_code)]
pub fn noctalia_settings_path() -> Option<PathBuf> {
    let paths = [
        state_dir().join("noctalia/settings.toml"),
        config_dir().join("noctalia/config.toml"),
    ];
    paths.into_iter().find(|path| Path::new(path).exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_valid_colors() {
        for data in [ThemeData::builtin_dark(), ThemeData::builtin_light()] {
            for color in [
                &data.background,
                &data.surface,
                &data.foreground,
                &data.accent,
                &data.selection,
                &data.danger,
            ] {
                assert!(valid_color(color), "{color} should be valid");
            }
            assert_eq!(data.tags.len(), 7);
            assert!((0.55..=1.0).contains(&data.opacity));
        }
    }

    #[test]
    fn theme_file_overrides_colors_and_clamps_opacity() {
        let base = ThemeData::builtin_dark();
        let file = ThemeFile::parse(
            r##"{
                "background": "#101010",
                "accent": "not-a-color",
                "opacity": 0.1,
                "radius": 99,
                "tags": ["#ff0000", "#00ff00", "#0000ff"]
            }"##,
        )
        .unwrap();
        let result = base.apply_file(file);
        assert_eq!(result.background, "#101010");
        assert_eq!(result.accent, base.accent);
        assert_eq!(result.opacity, 0.55);
        assert_eq!(result.radius, 24.0);
        assert_eq!(result.tags[0], "#ff0000");
        assert_eq!(result.tags[3], base.tags[3]);
    }

    #[test]
    fn font_size_has_a_backward_compatible_default_and_is_clamped() {
        let base = ThemeData::builtin_dark();
        assert_eq!(base.font_size, 12.0);
        let file = ThemeFile::parse(r#"{"fontSize": 40}"#).unwrap();
        assert_eq!(base.apply_file(file).font_size, 24.0);
        let file = ThemeFile::parse(r#"{"fontSize": 3}"#).unwrap();
        assert_eq!(base.apply_file(file).font_size, 9.0);
        let old_file = ThemeFile::parse(r#"{"fontFamily":"Inter"}"#).unwrap();
        assert_eq!(base.apply_file(old_file).font_size, 12.0);
    }

    #[test]
    fn appearance_overlay_loads_mode_and_keeps_custom_colors() {
        let appearance =
            ThemeFile::parse(r##"{"mode":"light","accent":"#123456","fontSize":15}"##).unwrap();
        let loaded = ThemeData::apply_appearance(
            LoadedTheme {
                data: ThemeData::builtin_dark(),
                warning: None,
                source: "built-in".to_owned(),
            },
            appearance,
        );
        assert_eq!(loaded.data.mode, "light");
        assert_eq!(
            loaded.data.background,
            ThemeData::builtin_light().background
        );
        assert_eq!(loaded.data.accent, "#123456");
        assert_eq!(loaded.data.font_size, 15.0);
        assert_eq!(loaded.source, "Filem appearance");
    }

    #[test]
    fn appearance_values_are_validated_and_clamped() {
        assert_eq!(
            validated_appearance_value("opacity", "0.1"),
            Some(serde_json::json!(0.55))
        );
        assert_eq!(
            validated_appearance_value("fontSize", "40"),
            Some(serde_json::json!(24.0))
        );
        assert_eq!(
            validated_appearance_value("fontFamily", "  Inter  "),
            Some(serde_json::json!("Inter"))
        );
        assert!(validated_appearance_value("background", "blue").is_none());
        assert!(validated_appearance_value("mode", "system").is_none());
        assert!(validated_appearance_value("tag8", "#ffffff").is_none());
    }

    #[test]
    fn appearance_persistence_preserves_other_settings_and_all_tag_defaults() {
        let directory = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("appearance-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("appearance.json");
        std::fs::write(&path, r#"{"unrelated":{"enabled":true}}"#).unwrap();
        let defaults = ThemeData::builtin_dark().tags;
        write_appearance_value(
            &path,
            "tag7",
            validated_appearance_value("tag7", "#123456").unwrap(),
            &defaults,
        )
        .unwrap();
        let document: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(document["unrelated"]["enabled"], true);
        assert_eq!(document["tags"].as_array().unwrap().len(), 7);
        assert_eq!(document["tags"][6], "#123456");
        let parsed = ThemeFile::parse(&document.to_string()).unwrap();
        assert_eq!(
            ThemeData::builtin_dark().apply_file(parsed).tags[6],
            "#123456"
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn camel_case_theme_fields_are_accepted() {
        let file = ThemeFile::parse(
            r##"{
                "surfaceAlt": "#222222",
                "selectionText": "#ffffff",
                "rowHeight": 42,
                "fontFamily": "Inter",
                "monoFont": "Iosevka"
            }"##,
        )
        .unwrap();
        let result = ThemeData::builtin_dark().apply_file(file);
        assert_eq!(result.surface_alt, "#222222");
        assert_eq!(result.selection_text, "#ffffff");
        assert_eq!(result.row_height, 42.0);
        assert_eq!(result.font_family, "Inter");
        assert_eq!(result.mono_font, "Iosevka");
    }

    #[test]
    fn invalid_theme_json_is_rejected() {
        assert!(ThemeFile::parse("{not json}").is_err());
    }

    #[test]
    fn manual_theme_toggle_persists_until_reload() {
        let temp = tempfile::tempdir().unwrap();
        let old_home = std::env::var_os("HOME");
        std::env::set_var("HOME", temp.path());
        std::env::set_var("XDG_CONFIG_HOME", temp.path().join("config"));
        std::env::set_var("XDG_STATE_HOME", temp.path().join("state"));
        let config_dir = temp.path().join("config/filem");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("theme.json"), r#"{"mode":"dark"}"#).unwrap();
        std::fs::write(
            config_dir.join("appearance.json"),
            r#"{"mode":"dark","opacity":0.85,"fontSize":14}"#,
        )
        .unwrap();

        let mut theme = Theme {
            data: ThemeData::builtin_dark(),
            ..Theme::default()
        };
        theme.toggle_mode();
        assert_eq!(theme.data.mode, "light");
        assert_eq!(theme.data.opacity, 0.85);
        assert_eq!(theme.data.font_size, 14.0);
        theme.reload();
        assert_eq!(theme.data.mode, "light");
        assert_eq!(theme.data.opacity, 0.85);
        assert_eq!(theme.data.font_size, 14.0);

        if let Some(value) = old_home {
            std::env::set_var("HOME", value);
        } else {
            std::env::remove_var("HOME");
        }
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::remove_var("XDG_STATE_HOME");
    }

    #[test]
    fn noctalia_variant_mapping_uses_terminal_tags() {
        let palette = NoctaliaVariant {
            surface: Some("#111111".to_owned()),
            on_surface: Some("#eeeeee".to_owned()),
            primary: Some("#88aaff".to_owned()),
            terminal: Some(NoctaliaTerminal {
                normal: Some(HashMap::from([
                    ("red".to_owned(), "#ff0000".to_owned()),
                    ("green".to_owned(), "#00ff00".to_owned()),
                    ("blue".to_owned(), "#0000ff".to_owned()),
                    ("yellow".to_owned(), "#ffff00".to_owned()),
                    ("magenta".to_owned(), "#ff00ff".to_owned()),
                    ("white".to_owned(), "#ffffff".to_owned()),
                ])),
                bright: Some(HashMap::new()),
            }),
            ..Default::default()
        };
        let result = ThemeData::builtin_dark().apply_noctalia(palette);
        assert_eq!(result.background, "#111111");
        assert_eq!(result.foreground, "#eeeeee");
        assert_eq!(result.tags[0], "#ff0000");
        assert_eq!(result.tags[6], "#ffffff");
    }
}
