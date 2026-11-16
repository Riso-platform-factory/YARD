//! The path of the saved window bounds.

use crate::Paths;
use std::path::PathBuf;

impl Paths {
    /// The file where the main window's place, size, and screen are saved.
    pub fn window_file(&self) -> PathBuf {
        self.data_dir().join("window.json")
    }
}

#[cfg(test)]
mod tests {
    use crate::Paths;
    use std::path::Path;

    #[test]
    fn window_file_is_in_the_data_directory() {
        let paths = Paths::under("given");
        assert_eq!(paths.window_file(), Path::new("given/data/window.json"));
    }
}
