//! What the details window remembers between runs: the chosen views and
//! the sensor branches the user closed.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use pulsar_core::paths;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CoreView {
    #[default]
    Matrix,
    Strips,
    Rings,
    Graphs,
}

impl CoreView {
    pub const ALL: [CoreView; 4] = [
        CoreView::Matrix,
        CoreView::Strips,
        CoreView::Rings,
        CoreView::Graphs,
    ];
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryView {
    #[default]
    Rings,
    Stick,
}

impl MemoryView {
    pub const ALL: [MemoryView; 2] = [MemoryView::Rings, MemoryView::Stick];
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub cores: CoreView,
    pub memory: MemoryView,
    /// Ids of the closed sensor branches.
    pub closed: BTreeSet<String>,
}

impl Prefs {
    /// A missing or unreadable file means the defaults.
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let text = toml::to_string(self).map_err(io::Error::other)?;
        fs::write(path, text)
    }
}

pub fn path() -> Option<PathBuf> {
    paths::data_dir().map(|dir| dir.join("details.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_tolerates_junk() {
        let dir = std::env::temp_dir().join(format!("pulsar-prefs-{}", std::process::id()));
        let file = dir.join("details.toml");
        let prefs = Prefs {
            cores: CoreView::Rings,
            memory: MemoryView::Stick,
            closed: ["cpu/Voltages".to_string()].into(),
        };
        prefs.save(&file).unwrap();
        assert_eq!(Prefs::load(&file), prefs);
        fs::write(&file, "cores = 7").unwrap();
        assert_eq!(Prefs::load(&file), Prefs::default());
        let _ = fs::remove_dir_all(dir);
    }
}
