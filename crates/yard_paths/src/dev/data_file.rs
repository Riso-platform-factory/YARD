use crate::Paths;
use std::path::PathBuf;

impl Paths {
    /// The application's database, `<data>/data.sqlite`.
    pub fn data_file(&self) -> PathBuf {
        let _group = tracing::info_span!("data-file").entered();
        let file = self.data_dir().join("data.sqlite");
        tracing::debug!(path = %file.display(), "resolved");
        file
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn data_file_is_in_the_data_directory() {
        assert_eq!(
            Paths::under("given").data_file(),
            Path::new("given/data/data.sqlite")
        );
    }
}
