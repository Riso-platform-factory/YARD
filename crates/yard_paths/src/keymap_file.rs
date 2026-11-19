//! The path of the user's keymap file.

use crate::Paths;
use std::path::PathBuf;

impl Paths {
    /// The user's keymap file.
    pub fn keymap_file(&self) -> PathBuf {
        self.config_dir().join("keymap.json")
    }
}

#[cfg(test)]
mod tests {
    use crate::Paths;
    use std::path::Path;

    #[test]
    fn keymap_file_is_in_the_config_directory() {
        let paths = Paths::under("given");
        assert_eq!(paths.keymap_file(), Path::new("given/config/keymap.json"));
    }
}
