//! The path of the saved session.

use crate::Paths;
use std::path::PathBuf;

impl Paths {
    /// The file where the windows open at quit are saved.
    pub fn session_file(&self) -> PathBuf {
        self.data_dir().join("session.json")
    }
}

#[cfg(test)]
mod tests {
    use crate::Paths;
    use std::path::Path;

    #[test]
    fn session_file_is_in_the_data_directory() {
        let paths = Paths::under("given");
        assert_eq!(paths.session_file(), Path::new("given/data/session.json"));
    }
}
