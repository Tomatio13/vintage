//! Preview-only preferences. Call disk operations outside the GPUI thread.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

pub const SCROLLBACK: [usize; 4] = [1000, 2500, 5000, 10000];
pub const FONT_PRESETS: [&str; 6] = [
    "Default",
    "Cica",
    "HackGen",
    "HackGen35",
    "JetBrainsMono Nerd Font",
    "Custom",
];
pub const ACTIONS: [&str; 9] = [
    "Previous tab",
    "Next tab",
    "Previous pane",
    "Next pane",
    "Previous workspace",
    "Next workspace",
    "New terminal",
    "Split right",
    "Split down",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
    Graphite,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub key: String,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}
impl Binding {
    pub fn label(&self) -> String {
        format!(
            "{}{}{}{}",
            if self.ctrl { "Ctrl+" } else { "" },
            if self.alt { "Alt+" } else { "" },
            if self.shift { "Shift+" } else { "" },
            self.key
        )
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.key.is_empty() && self.key.len() <= 32 && !self.key.chars().any(char::is_control),
            "Invalid shortcut key"
        );
        ensure!(self.ctrl || self.alt, "Use Ctrl or Alt in a shortcut");
        ensure!(
            !matches!(
                self.key.as_str(),
                "escape" | "control" | "shift" | "alt" | "super" | "cmd"
            ),
            "This key cannot be assigned"
        );
        ensure!(
            !(self.ctrl
                && !self.alt
                && ((self.shift
                    && matches!(self.key.as_str(), "f" | "o" | "w" | "tab" | "c" | "v"))
                    || matches!(self.key.as_str(), "," | "+" | "=" | "-" | "0"))),
            "This shortcut is reserved by VINTAGE"
        );
        Ok(())
    }
}
pub fn default_bindings() -> [Binding; 9] {
    std::array::from_fn(|i| Binding {
        key: [
            "left", "right", "up", "down", "left", "right", "n", "d", "t",
        ][i]
            .into(),
        ctrl: i != 4 && i != 5,
        alt: i == 4 || i == 5,
        shift: true,
    })
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub appearance: Appearance,
    pub ui_percent: u16,
    pub font_preset: usize,
    pub custom_font: String,
    pub font_size: u16,
    pub scrollback: usize,
    pub shell: String,
    pub bindings: [Binding; 9],
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            appearance: Appearance::System,
            ui_percent: 100,
            font_preset: 0,
            custom_font: String::new(),
            font_size: 12,
            scrollback: 1000,
            shell: crate::default_shell_id().into(),
            bindings: default_bindings(),
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "Unsupported Preview settings version");
        ensure!(
            (85..=200).contains(&self.ui_percent) && self.ui_percent.is_multiple_of(5),
            "UI text size must be 85–200% in 5% steps"
        );
        ensure!(
            (8..=48).contains(&self.font_size),
            "Terminal font size must be 8–48 px"
        );
        ensure!(
            SCROLLBACK.contains(&self.scrollback),
            "Unsupported scrollback capacity"
        );
        ensure!(
            self.font_preset < FONT_PRESETS.len()
                && self.custom_font.len() <= 1024
                && !self.custom_font.chars().any(char::is_control),
            "Invalid font preference"
        );
        ensure!(
            self.font_preset != 5 || !self.custom_font.trim().is_empty(),
            "Enter a custom font name"
        );
        ensure!(
            !self.shell.is_empty()
                && self.shell.len() <= 4096
                && !self.shell.chars().any(char::is_control),
            "Invalid shell preference"
        );
        for (i, binding) in self.bindings.iter().enumerate() {
            binding.validate()?;
            ensure!(
                !self.bindings[..i].contains(binding),
                "Shortcut is already assigned"
            );
        }
        Ok(())
    }
    pub fn font_family(&self) -> &str {
        match self.font_preset {
            0 => {
                if cfg!(windows) {
                    "Consolas"
                } else {
                    "DejaVu Sans Mono"
                }
            }
            5 => self.custom_font.trim(),
            i => FONT_PRESETS.get(i).copied().unwrap_or("monospace"),
        }
    }
    pub fn rebind(&mut self, index: usize, binding: Binding) -> Result<()> {
        ensure!(index < self.bindings.len(), "Unknown shortcut action");
        binding.validate()?;
        ensure!(
            !self
                .bindings
                .iter()
                .enumerate()
                .any(|(i, b)| i != index && b == &binding),
            "Shortcut is already assigned"
        );
        self.bindings[index] = binding;
        Ok(())
    }
}
#[derive(Clone)]
pub struct SettingsStore {
    path: PathBuf,
}
impl SettingsStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
    pub fn default_location() -> Result<Self> {
        let base = if cfg!(windows) {
            std::env::var_os("APPDATA").map(PathBuf::from)
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
        };
        let base = base.context("Cannot locate the Preview settings directory")?;
        Ok(Self::new(
            base.join("vintage-gpui-preview").join("settings.json"),
        ))
    }
    pub fn load(&self) -> Result<Settings> {
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
            Err(e) => return Err(e).context("Cannot inspect Preview settings"),
        };
        ensure!(metadata.is_file(), "Settings must be a regular file");
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let file = match options.open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
            Err(e) => return Err(e).context("Cannot read Preview settings"),
        };
        ensure!(
            file.metadata()?.is_file(),
            "Settings must be a regular file"
        );
        let mut bytes = Vec::new();
        file.take(65537).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 65536, "Settings file exceeds 64 KiB");
        let mut value: serde_json::Value = serde_json::from_slice(&bytes)
            .context("Preview settings are damaged; defaults are active until you save")?;
        if let Some(bindings) = value
            .get_mut("bindings")
            .and_then(serde_json::Value::as_array_mut)
        {
            if bindings.len() == 6 {
                let defaults = default_bindings();
                bindings.extend(
                    defaults[6..]
                        .iter()
                        .map(|binding| serde_json::json!(binding)),
                );
            }
        }
        let settings: Settings = serde_json::from_value(value)
            .context("Preview settings are damaged; defaults are active until you save")?;
        settings.validate()?;
        Ok(settings)
    }
    pub fn save(&self, settings: &Settings) -> Result<()> {
        settings.validate()?;
        let parent = self
            .path
            .parent()
            .context("Settings path needs a directory")?;
        fs::create_dir_all(parent).context("Cannot create Preview settings directory")?;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let temporary = parent.join(format!(
            ".settings-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| -> Result<()> {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(&serde_json::to_vec_pretty(settings)?)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, &self.path).context("Cannot replace Preview settings")?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shortcuts_reject_conflicts_reserved_and_plain_typing() {
        let mut s = Settings::default();
        let original = s.clone();
        assert!(s.rebind(0, s.bindings[1].clone()).is_err());
        assert!(s
            .rebind(
                0,
                Binding {
                    key: "t".into(),
                    ctrl: true,
                    alt: false,
                    shift: true
                }
            )
            .is_err());
        assert!(s
            .rebind(
                0,
                Binding {
                    key: "x".into(),
                    ctrl: false,
                    alt: false,
                    shift: false
                }
            )
            .is_err());
        assert_eq!(s, original);
        s.rebind(
            0,
            Binding {
                key: "f6".into(),
                ctrl: true,
                alt: false,
                shift: false,
            },
        )
        .unwrap();
        s.validate().unwrap();
    }
    #[test]
    fn failed_save_preserves_destination_and_cleans_temporary_file() {
        let root =
            std::env::temp_dir().join(format!("vintage-settings-failure-{}", std::process::id()));
        fs::create_dir_all(root.join("settings.json")).unwrap();
        let store = SettingsStore::new(root.join("settings.json"));
        assert!(store.save(&Settings::default()).is_err());
        assert!(store.path.is_dir());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn bounds_and_future_versions_are_rejected() {
        let mut s = Settings::default();
        s.validate().unwrap();
        s.ui_percent = 201;
        assert!(s.validate().is_err());
        s.ui_percent = 200;
        s.font_size = 49;
        assert!(s.validate().is_err());
        s.font_size = 48;
        s.scrollback = 10001;
        assert!(s.validate().is_err());
        s.scrollback = 10000;
        s.version = 2;
        assert!(s.validate().is_err());
    }
    #[test]
    fn six_binding_settings_migrate_with_default_terminal_actions() {
        let root =
            std::env::temp_dir().join(format!("vintage-settings-migrate-{}", std::process::id()));
        let store = SettingsStore::new(root.join("settings.json"));
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value["bindings"].as_array_mut().unwrap().truncate(6);
        fs::create_dir_all(&root).unwrap();
        fs::write(&store.path, serde_json::to_vec(&value).unwrap()).unwrap();
        let settings = store.load().unwrap();
        assert_eq!(settings.bindings[6].label(), "Ctrl+Shift+n");
        assert_eq!(settings.bindings[7].label(), "Ctrl+Shift+d");
        assert_eq!(settings.bindings[8].label(), "Ctrl+Shift+t");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn save_reload_replace_and_damaged_recovery() {
        let root =
            std::env::temp_dir().join(format!("vintage-settings-test-{}", std::process::id()));
        let store = SettingsStore::new(root.join("settings.json"));
        assert_eq!(store.load().unwrap(), Settings::default());
        let mut s = Settings::default();
        store.save(&s).unwrap();
        s.font_size = 20;
        s.appearance = Appearance::Light;
        store.save(&s).unwrap();
        assert_eq!(store.load().unwrap(), s);
        fs::write(&store.path, b"broken").unwrap();
        assert!(store.load().is_err());
        assert_eq!(fs::read(&store.path).unwrap(), b"broken");
        store.save(&s).unwrap();
        assert_eq!(store.load().unwrap(), s);
        fs::write(&store.path, vec![b' '; 65537]).unwrap();
        assert!(store.load().is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
