use std::{env, ffi::OsStr, fs::read_dir, io, path::Path};
use bindings::db::DatabaseImpl;
use libloading::{Library, Symbol};
use ouroboros::self_referencing;
use crate::{app::router::Router, utils::log::{log_error, log_info}};


type InitModuleFn = extern "C" fn ();
type ProvideDatabaseFn = fn () -> Box<dyn DatabaseImpl>;
type ProvideModelsFn = extern "C" fn ();
type ProvideRoutesFn = extern "C" fn (router: *mut Router);

#[self_referencing]
pub struct Module {
	name: String,
	lib: Library,

	#[borrows(lib)]
	#[covariant]
	init_module: Option<Symbol<'this, InitModuleFn>>,
	#[borrows(lib)]
	#[covariant]
	provide_database: Option<Symbol<'this, ProvideDatabaseFn>>,
	#[borrows(lib)]
	#[covariant]
	provide_models: Option<Symbol<'this, ProvideModelsFn>>,
	#[borrows(lib)]
	#[covariant]
	provide_routes: Option<Symbol<'this, ProvideRoutesFn>>,
}

impl Module {
	pub fn load<P: AsRef<OsStr>> (name: &str, path: P) -> Result<Self, libloading::Error> {
		let builder = ModuleBuilder {
			name: name.to_owned(),
			lib: unsafe { Library::new(path) }?,
			init_module_builder: |lib: &Library| unsafe { lib.get(b"init_module") }.ok(),
			provide_database_builder: |lib: &Library| unsafe { lib.get(b"provide_database") }.ok(),
			provide_models_builder: |lib: &Library| unsafe { lib.get(b"provide_models") }.ok(),
			provide_routes_builder: |lib: &Library| unsafe { lib.get(b"provide_routes") }.ok(),
		};

		let module = builder.build();
		module.with_init_module(|symbol| {
			if let Some(call) = symbol {
                log_info(&format!("{}: calling init", name));
                call();
            }
		});

		log_info(&format!("{name}: loaded"));
		return Ok(module);
	}

	#[inline]
	pub fn get_name (&self) -> &str {
		return &self.borrow_name();
	}

	pub fn provide_database (&self) -> Option<Box<dyn DatabaseImpl>> {
		self.with_provide_database(|symbol| {
            if let Some(call) = symbol {
				let name = self.get_name();
                log_info(&format!("{}: calling provide_database", name));
                return Some(call());
            } else {
				return None;
			}
        })
	}

	pub fn provide_models (&self) {
		self.with_provide_models(|symbol| {
            if let Some(call) = symbol {
				let name = self.get_name();
                log_info(&format!("{}: calling provide_models", name));
                call();
            }
        });
	}

	pub fn provide_routes (&self, router: &mut Router) {
		self.with_provide_routes(|symbol| {
            if let Some(call) = symbol {
				let name = self.get_name();
                log_info(&format!("{}: calling provide_routes", name));
                router.with_module(name, |router| call(router));
            }
        });
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
