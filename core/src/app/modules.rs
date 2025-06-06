use std::{env, ffi::OsStr, fs::read_dir, io, path::Path};
use libloading::{Library, Symbol};
use ouroboros::self_referencing;
use crate::{app::router::Router, utils::log::{log_error, log_info}};


type OnAttachHandler = extern "C" fn (router: *mut Router);

#[self_referencing]
pub struct Module {
	name: String,
	lib: Library,
	#[borrows(lib)]
	#[covariant]
	pub on_attach: Option<Symbol<'this, OnAttachHandler>>
}

impl Module {
	pub fn load<P: AsRef<OsStr>> (name: &str, path: P) -> Result<Self, libloading::Error> {
		let builder = ModuleBuilder {
			name: name.to_owned(),
			lib: unsafe { Library::new(path) }?,
			on_attach_builder: |lib: &Library| unsafe { lib.get(b"on_attach") }.ok()
		};

		log_info(&format!("{name}: loaded"));
		return Ok(builder.build());
	}

	#[inline]
	pub fn get_name (&self) -> &str {
		return &self.borrow_name();
	}
}


pub fn load_modules<P: AsRef<Path>> (modules: &mut Vec<Module>, base_dir: P) -> io::Result<()> {
	let ext = match env::consts::OS {
		"linux" => ".so",
		"windows" => ".dll",
		"macos" => ".dylib",
		_ => {
			return Err(io::Error::other("Unsupported OS"));
		}
	};

	for dir in read_dir(base_dir)? {
		let path = dir?.path();
		let mut name = path.file_name().unwrap().to_str().unwrap();

		if !name.ends_with(ext) {
			continue;
		}

		name = &name[0..(name.len() - ext.len())];

		match Module::load(name, &path) {
			Err(error) => {
				log_error(&format!("{}: failed to load, {error}", name));
			},
			Ok(module) => {
				modules.push(module);
			}
		}
	}

	return Ok(());
}
