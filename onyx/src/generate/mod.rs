use std::{collections::BTreeMap, env, fs, io::IsTerminal, path::{Path, PathBuf}, thread, time::Duration};
use photonyx_models::{load_groups, to_runtime};
use crate::{config, log::Log};

mod zig;


// TODO: remove the delay hack on a next ZLS release
const DEFAULT_ZLS_DELAY_MS: u64 = 500;

/// Version of the arguments passed by the build helper (`photonyx.zig/build.zig`, `onyx_protocol`)
const PROTOCOL: &str = "2";

const ERRORS_BEGIN: &str = "// onyx: model errors\n";
const ERRORS_END: &str = "// onyx: end of model errors\n";

struct Options {
	zls_delay: Duration,
	project: PathBuf,
	out_dir: PathBuf,
	dep_file: PathBuf
}

fn parse_args (args: &[&str]) -> Result<Options, String> {
	let mut lang = None;
	let mut project = PathBuf::from("photonyx.json");

	let mut zls_delay = match env::var("ONYX_ZLS_DELAY") {
		Ok(raw) => raw.parse().map_err(|_| format!("ONYX_ZLS_DELAY: expected milliseconds, got `{raw}`"))?,
		Err(_) => DEFAULT_ZLS_DELAY_MS
	};

	let mut positional = Vec::new();

	let mut iter = args.iter();
	while let Some(arg) = iter.next() {
		match *arg {
			"--lang" => lang = Some(*iter.next().ok_or("--lang: value expected")?),
			"--project" => project = PathBuf::from(*iter.next().ok_or("--project: value expected")?),
			"--zls-delay" => {
				let raw = iter.next().ok_or("--zls-delay: value expected")?;
				zls_delay = raw.parse().map_err(|_| format!("--zls-delay: expected milliseconds, got `{raw}`"))?;
			}
			"--protocol" => {
				let protocol = iter.next().ok_or("--protocol: value expected")?;
				if *protocol != PROTOCOL {
					return Err(format!(
						"the build helper uses protocol {protocol}, this onyx supports {PROTOCOL}: update {}",
						if protocol.parse::<u32>().unwrap_or(0) > PROTOCOL.parse::<u32>().unwrap() { "onyx" } else { "the photonyx package (zig fetch)" }
					));
				}
			}
			// A directory input for `zig build --watch` (new files in a group), not used here
			_ if arg.starts_with("--watch-dir=") => {}
			_ if arg.starts_with("--") => return Err(format!("unknown option {arg}")),
			_ => positional.push(*arg)
		}
	}

	let lang = lang.ok_or("--lang is required (zig)")?;
	if lang != "zig" {
		return Err(format!("unsupported language `{lang}`, only zig is available"));
	}

	return match positional.as_slice() {
		[out_dir, dep_file] => Ok(Options {
			zls_delay: Duration::from_millis(zls_delay),
			project,
			out_dir: PathBuf::from(out_dir),
			dep_file: PathBuf::from(dep_file)
		}),
		_ => Err("expected <out-dir> <depfile>".to_owned())
	};
}

pub fn run (args: &[&str]) -> i32 {
	let options = match parse_args(args) {
		Ok(options) => options,
		Err(error) => {
			Log::error_full(&format!("models generate: {error}"), &crate::usage_by_prefix("models generate"));
			return 2;
		}
	};

	match generate(&options) {
		Ok(()) => return 0,
		Err(error) => {
			Log::error_full("models generate failed", &error);
			return 1;
		}
	}
}

fn generate (options: &Options) -> Result<(), String> {
	let groups = config::model_groups(&config::load_project(&options.project)?)?;

	let mut loaded = load_groups(&groups);
	loaded.files.insert(0, options.project.clone());

	if !loaded.errors.is_empty() {
		let messages: Vec<String> = loaded.errors.iter().map(|error| error.to_string()).collect();

		// Skip error printing when running in a build tool
		if std::io::stderr().is_terminal() {
			Log::error_full("models generate: errors in models", &messages.join("\n"));
		}

		return generate_errors(options, &groups, &loaded.files, &messages);
	}

	let document = to_runtime(&loaded.set);

	// Relative path in the output dir --> File content
	let mut files: BTreeMap<PathBuf, String> = zig::render(&loaded.set, &source_root());
	files.insert(PathBuf::from("schema.json"), document.to_canonical());

	write_output(&options.out_dir, &files, options.zls_delay, true)?;
	write_dep_file(&options.dep_file, &loaded.files)?;
	return Ok(());
}

/// Model errors become compile errors of the `db` module instead of a failed build step:
/// - the step succeeds and writes its depfile, so `zig build --watch` knows the model files
///   and re-runs the step once they are fixed;
/// - the build fails with the model error, and ZLS shows it through build-on-save;
/// - the bindings of the last successful run are kept, so completions keep working.
fn generate_errors (options: &Options, groups: &[(String, PathBuf)], model_files: &[PathBuf], messages: &[String]) -> Result<(), String> {
	let db_path = options.out_dir.join("db.zig");
	let previous = fs::read_to_string(&db_path).ok().map(|content| without_errors(&content));

	let mut block = String::from(ERRORS_BEGIN);
	for message in messages {
		block.push_str(&format!("comptime {{\n    @compileError({});\n}}\n", zig::string_literal(message)));
	}
	block.push_str(ERRORS_END);

	let content = match previous {
		Some(previous) => format!("{block}\n{previous}"),
		None => format!("{block}\n{}", placeholders(groups))
	};

	let files = BTreeMap::from([(PathBuf::from("db.zig"), content)]);
	write_output(&options.out_dir, &files, options.zls_delay, false)?;
	write_dep_file(&options.dep_file, model_files)?;
	return Ok(());
}

fn placeholders (groups: &[(String, PathBuf)]) -> String {
	let entries = |dir: &Path| -> Vec<PathBuf> {
		let mut paths: Vec<PathBuf> = fs::read_dir(dir)
			.map(|entries| entries.filter_map(|entry| entry.ok().map(|entry| entry.path())).collect())
			.unwrap_or_default();

		paths.sort();
		paths
	};

	let name = |path: &Path| path.file_stem().map(|stem| format!("@{}", zig::string_literal(&stem.to_string_lossy()))).unwrap_or_default();
	let ident = |name: &str| format!("@{}", zig::string_literal(name));
	let error = zig::string_literal("model bindings are not generated yet: fix the model errors above");

	let mut result = String::new();
	for (group, dir) in groups {
		result.push_str(&format!("pub const {} = struct {{\n", ident(group)));
		for file in entries(dir).iter().filter(|path| path.extension().is_some_and(|ext| ext == "yml")) {
			result.push_str(&format!("    pub const {} = @compileError({error});\n", name(file)));
		}
		result.push_str("};\n");
	}

	return result;
}

fn without_errors (content: &str) -> String {
	if let Some(rest) = content.strip_prefix(ERRORS_BEGIN) {
		if let Some(end) = rest.find(ERRORS_END) {
			return rest[end + ERRORS_END.len()..].trim_start_matches('\n').to_owned();
		}
	}

	return content.to_owned();
}

fn source_root () -> PathBuf {
	return env::current_dir().unwrap_or_default();
}

fn write_output (out_dir: &Path, files: &BTreeMap<PathBuf, String>, zls_delay: Duration, remove_stale: bool) -> Result<(), String> {
	let mut pending = Vec::new();
	let mut replaced = Vec::new();

	for (relative, content) in files {
		let path = out_dir.join(relative);
		match fs::read(&path) {
			Ok(existing) => {
				if existing == content.as_bytes() {
					continue;
				}

				replaced.push(path.clone());
			}
			Err(_) => {}
		}

		pending.push((path, content));
	}

	let stale = if remove_stale { find_stale(out_dir, files)? } else { Vec::new() };

	if !zls_delay.is_zero() && !replaced.is_empty() {
		for path in &replaced {
			fs::remove_file(path).map_err(|error| format!("{}: {error}", path.display()))?;
		}

		thread::sleep(zls_delay);
	}

	for (path, content) in pending {
		if let Some(parent) = path.parent() {
			fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
		}

		fs::write(&path, content).map_err(|error| format!("{}: {error}", path.display()))?;
	}

	for path in stale {
		fs::remove_file(&path).map_err(|error| format!("{}: {error}", path.display()))?;

		if let Some(parent) = path.parent() {
			if parent != out_dir && fs::read_dir(parent).is_ok_and(|mut entries| entries.next().is_none()) {
				let _ = fs::remove_dir(parent);
			}
		}
	}

	return Ok(());
}

fn find_stale (out_dir: &Path, files: &BTreeMap<PathBuf, String>) -> Result<Vec<PathBuf>, String> {
	let mut result = Vec::new();
	let mut dirs = vec![out_dir.to_owned()];

	while let Some(dir) = dirs.pop() {
		let entries = match fs::read_dir(&dir) {
			Ok(entries) => entries,
			Err(_) => continue
		};

		for entry in entries {
			let path = entry.map_err(|error| format!("{}: {error}", dir.display()))?.path();
			if path.is_dir() {
				dirs.push(path);
				continue;
			}

			let generated = path.extension().is_some_and(|ext| ext == "zig" || ext == "json");
			let relative = path.strip_prefix(out_dir).unwrap_or(&path);
			if generated && !files.contains_key(relative) {
				result.push(path);
			}
		}
	}

	result.sort();
	return Ok(result);
}

fn write_dep_file (dep_file: &Path, inputs: &[PathBuf]) -> Result<(), String> {
	let paths: Vec<PathBuf> = inputs.iter().map(|path| fs::canonicalize(path).unwrap_or_else(|_| path.clone())).collect();

	let mut content = String::from("models:");
	for path in paths {
		content.push(' ');
		content.push_str(&path.to_string_lossy().replace(' ', "\\ "));
	}
	content.push('\n');

	if let Some(parent) = dep_file.parent() {
		let _ = fs::create_dir_all(parent);
	}

	return fs::write(dep_file, content).map_err(|error| format!("{}: {error}", dep_file.display()));
}
