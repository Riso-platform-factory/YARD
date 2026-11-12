//! The path of the user settings file.

use crate::Paths;
use std::path::PathBuf;

impl Paths {
    /// The file where the user's settings are read.
    pub fn settings_file(&self) -> PathBuf {
        self.config_dir().join("settings.json")
    }
}

#[cfg(test)]
mod tests {
    use crate::Paths;
    use std::path::Path;

    #[test]
    fn settings_file_is_in_the_config_directory() {
        let paths = Paths::under("given");
        assert_eq!(
            paths.settings_file(),
            Path::new("given/config/settings.json")
        );
    }
}
