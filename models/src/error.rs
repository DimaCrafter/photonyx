use std::{fmt, path::{Path, PathBuf}};
use crate::ir::Pos;


#[derive(Debug, Clone, PartialEq)]
pub struct ModelError {
	pub file: PathBuf,
	pub pos: Option<Pos>,
	pub message: String
}

impl ModelError {
	pub fn at (file: &Path, pos: Pos, message: String) -> Self {
		ModelError { file: file.to_owned(), pos: Some(pos), message }
	}

	pub fn file (file: &Path, message: String) -> Self {
		ModelError { file: file.to_owned(), pos: None, message }
	}
}

impl fmt::Display for ModelError {
	fn fmt (&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self.pos {
			Some(pos) => write!(f, "{}:{}:{}: {}", self.file.display(), pos.line, pos.col, self.message),
			None => write!(f, "{}: {}", self.file.display(), self.message)
		}
	}
}
