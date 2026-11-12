use std::{fs, io, path::PathBuf};

/// The last observed state of a file.
#[derive(Clone, Debug, PartialEq)]
pub enum FileState {
    Missing,
    Bytes(Vec<u8>),
    Unreadable(String),
}

/// Compares a file's complete bytes with its previous state.
pub struct FileCheck {
    path: PathBuf,
    last: Option<FileState>,
}

impl FileCheck {
    /// Creates a check without reading the path.
    pub fn new(path: PathBuf) -> Self {
        Self { path, last: None }
    }

    /// Returns the state only on the first check or when it changes.
    pub fn check(&mut self) -> Option<FileState> {
        let state = match fs::read(&self.path) {
            Ok(bytes) => FileState::Bytes(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => FileState::Missing,
            Err(error) => FileState::Unreadable(error.to_string()),
        };
        tracing::trace!(path = %self.path.display(), "checked");
        if self.last.as_ref() == Some(&state) {
            None
        } else {
            self.last = Some(state.clone());
            Some(state)
        }
    }
}

#[cfg(test)]
mod tests;
