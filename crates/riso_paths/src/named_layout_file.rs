//! Paths of named layouts' saved user layers.
use crate::Paths;
use std::path::PathBuf;

impl Paths {
    /// The file where a named layout's user operations are saved.
    pub fn named_layout_file(&self, name: &str) -> PathBuf {
        self.data_dir().join("layouts").join(format!("{name}.json"))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn named_layout_file_is_in_the_data_directory() {
        let paths = crate::Paths::under("given");
        assert_eq!(
            paths.named_layout_file("layout_tour.swap"),
            std::path::Path::new("given/data/layouts/layout_tour.swap.json")
        );
    }
}
