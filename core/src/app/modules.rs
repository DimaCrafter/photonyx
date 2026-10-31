use std::{env, ffi::{c_char, OsStr}, fs::read_dir, io, path::Path, sync::Arc};
use bindings::{db::DatabaseImpl, rs::RustFnLinks, session::SessionStoreMeta};
use libloading::{Library, Symbol};
use ouroboros::self_referencing;
use photonyx_models::runtime::RuntimeModels;
use crate::{app::router::Router, c::c_string, db::{connection::rs_find_db_connection, model::{with_attach_context, AttachContext}}, utils::log::{log_error, log_info}};


type RsLinkFn = extern "C" fn (links: &'static RustFnLinks);
type InitModuleFn = extern "C" fn ();
type ProvideDatabaseFn = fn () -> Box<dyn DatabaseImpl>;
type ProvideSessionStoreFn = fn () -> Box<dyn SessionStoreMeta>;
type ProvideModelsFn = extern "C" fn ();
type ProvideRoutesFn = extern "C" fn (router: *mut Router);
type ProvideMiddlewaresFn = extern "C" fn (router: *mut Router);
type StartupFn = extern "C" fn ();

static FN_LINKS: RustFnLinks = RustFnLinks {
	find_db_connection: rs_find_db_connection
};

#[self_referencing]
pub struct Module {
	name: String,
	/// Runtime description of models from the exported `models_schema`
	models: Option<Arc<RuntimeModels>>,
	lib: Library,

	#[borrows(lib)]
	#[covariant]
	rs_link: Option<Symbol<'this, RsLinkFn>>,
	#[borrows(lib)]
	#[covariant]
	init_module: Option<Symbol<'this, InitModuleFn>>,
	#[borrows(lib)]
	#[covariant]
	provide_database: Option<Symbol<'this, ProvideDatabaseFn>>,
	#[borrows(lib)]
	#[covariant]
	provide_session_store: Option<Symbol<'this, ProvideSessionStoreFn>>,
	#[borrows(lib)]
	#[covariant]
	provide_models: Option<Symbol<'this, ProvideModelsFn>>,
	#[borrows(lib)]
	#[covariant]
	provide_routes: Option<Symbol<'this, ProvideRoutesFn>>,
	#[borrows(lib)]
	#[covariant]
	provide_middlewares: Option<Symbol<'this, ProvideMiddlewaresFn>>,
	#[borrows(lib)]
	#[covariant]
	startup: Option<Symbol<'this, StartupFn>>,
}

impl Module {
	pub fn load<P: AsRef<OsStr>> (name: &str, path: P) -> Result<Self, String> {
		let lib = unsafe { Library::new(path) }.map_err(|error| error.to_string())?;
		let models = read_models_schema(&lib)?.map(Arc::new);

		let builder = ModuleBuilder {
			name: name.to_owned(),
			models,
			lib,
			rs_link_builder: |lib: &Library| unsafe { lib.get(b"rs_link") }.ok(),
			init_module_builder: |lib: &Library| unsafe { lib.get(b"init_module") }.ok(),
			provide_database_builder: |lib: &Library| unsafe { lib.get(b"provide_database") }.ok(),
			provide_session_store_builder: |lib: &Library| unsafe { lib.get(b"provide_session_store") }.ok(),
			provide_models_builder: |lib: &Library| unsafe { lib.get(b"provide_models") }.ok(),
			provide_routes_builder: |lib: &Library| unsafe { lib.get(b"provide_routes") }.ok(),
			provide_middlewares_builder: |lib: &Library| unsafe { lib.get(b"provide_middlewares") }.ok(),
			startup_builder: |lib: &Library| unsafe { lib.get(b"startup") }.ok(),
		};

		let module = builder.build();
		module.with_rs_link(|symbol| {
			if let Some(call) = symbol {
                log_info(&format!("{}: calling rs_link", name));
                call(&FN_LINKS);
            }
		});

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

	#[inline]
	pub fn get_models (&self) -> Option<&Arc<RuntimeModels>> {
		return self.borrow_models().as_ref();
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

	pub fn provide_session_store (&self) -> Option<Box<dyn SessionStoreMeta>> {
		self.with_provide_session_store(|symbol| {
            if let Some(call) = symbol {
				let name = self.get_name();
                log_info(&format!("{}: calling provide_session_store", name));
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

				let context = AttachContext {
					module: name.to_owned(),
					models: self.borrow_models().clone()
				};

                with_attach_context(context, || call());
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

	pub fn provide_middlewares (&self, router: &mut Router) {
		self.with_provide_middlewares(|symbol| {
            if let Some(call) = symbol {
				let name = self.get_name();
                log_info(&format!("{}: calling provide_middlewares", name));
                router.with_module(name, |router| call(router));
            }
        });
	}

	pub fn startup (&self) {
		self.with_startup(|symbol| {
            if let Some(call) = symbol {
				let name = self.get_name();
                log_info(&format!("{}: calling startup", name));
                call();
            }
        });
	}
}


fn read_models_schema (lib: &Library) -> Result<Option<RuntimeModels>, String> {
	let symbol: Symbol<*const *const c_char> = match unsafe { lib.get(b"models_schema") } {
		Ok(symbol) => symbol,
		Err(_) => return Ok(None)
	};

	let raw = unsafe { c_string(**symbol) };
	return RuntimeModels::parse(&raw).map(Some);
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
