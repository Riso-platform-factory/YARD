use crate::Paths;
use std::path::PathBuf;

impl Paths {
    /// The folder of the extensions' stores.
    pub fn store_dir(&self) -> PathBuf {
        let _group = tracing::info_span!("store-dir").entered();
        let dir = self.data_dir().join("store");
        tracing::debug!(path = %dir.display(), "resolved");
        dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn store_dir_is_in_the_data_directory() {
        assert_eq!(
            Paths::under("given").store_dir(),
            Path::new("given/data/store")
        );
    }
}
