//! The path of the saved user layout.

use crate::Paths;
use std::path::PathBuf;

impl Paths {
    /// The file where the user's layout operations are saved.
    pub fn layout_file(&self) -> PathBuf {
        self.data_dir().join("layout.json")
    }
}

#[cfg(test)]
mod tests {
    use crate::Paths;
    use std::path::Path;

    #[test]
    fn layout_file_is_in_the_data_directory() {
        let paths = Paths::under("given");
        assert_eq!(paths.layout_file(), Path::new("given/data/layout.json"));
    }
}
