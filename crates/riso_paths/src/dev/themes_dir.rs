//! The path of the user's themes folder.

use crate::Paths;
use std::path::PathBuf;

impl Paths {
    /// The folder of the user's theme files.
    pub fn themes_dir(&self) -> PathBuf {
        self.config_dir().join("themes")
    }
}

#[cfg(test)]
mod tests {
    use crate::Paths;
    use std::path::Path;

    #[test]
    fn themes_dir_is_in_the_config_directory() {
        let paths = Paths::under("given");
        assert_eq!(paths.themes_dir(), Path::new("given/config/themes"));
    }
}
