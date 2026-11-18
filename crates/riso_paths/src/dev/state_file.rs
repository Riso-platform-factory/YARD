//! The path of the saved node state.

use crate::Paths;
use std::path::PathBuf;

impl Paths {
    /// The file where the state of nodes is saved.
    pub fn state_file(&self) -> PathBuf {
        self.data_dir().join("state.json")
    }
}

#[cfg(test)]
mod tests {
    use crate::Paths;
    use std::path::Path;

    #[test]
    fn state_file_is_in_the_data_directory() {
        let paths = Paths::under("given");
        assert_eq!(paths.state_file(), Path::new("given/data/state.json"));
    }
}
