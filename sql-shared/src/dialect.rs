use std::collections::BTreeMap;
use json::JsonValue;
use photonyx_models::ir::{DialectBlock, Enum, Field, Index, Table};
use crate::{diff::Op, schema::{ColumnType, DbSchema, Generated}};


/// Field's dialect block options
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ColumnOptions {
	pub collation: Option<String>,
	pub default: Option<String>,
	pub generated: Option<Generated>
}

pub type ProvideSqlDialectFn = fn () -> Box<dyn SqlDialect>;

#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
	pub sql: String,
	pub op_idx: usize
}

pub type Row = Vec<SqlValue>;

pub trait SqlSession {
	fn query (&mut self, sql: &str, params: &[SqlValue]) -> Result<Vec<Row>, String>;
	fn exec (&mut self, sql: &str) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum SqlValue {
	Null,
	Int(i64),
	UInt(u64),
	Float(f64),
	Str(String),
	Bytes(Vec<u8>)
}

impl SqlValue {
	pub fn text (&self) -> Option<String> {
		return match self {
			SqlValue::Null => None,
			SqlValue::Int(value) => Some(value.to_string()),
			SqlValue::UInt(value) => Some(value.to_string()),
			SqlValue::Float(value) => Some(value.to_string()),
			SqlValue::Str(value) => Some(value.clone()),
			SqlValue::Bytes(value) => Some(String::from_utf8_lossy(value).into_owned())
		};
	}

	pub fn is_null (&self) -> bool {
		return *self == SqlValue::Null;
	}
}

pub trait SqlDialect {
	/// Driver ID, the same as `driver` in the config and the name of the dialect block in models
	fn id (&self) -> &str;
	fn map_type (&self, field: &Field, enum_entity: Option<&Enum>) -> Result<ColumnType, String>;
	fn column_options (&self, field: &Field) -> Result<ColumnOptions, String>;
	fn index_using (&self, index: &Index) -> Result<Option<String>, String>;
	fn table_options (&self, table: &Table, defaults: &[DialectBlock]) -> Result<BTreeMap<String, String>, String>;
	fn render (&self, ops: &[Op]) -> Vec<Statement>;
	/// Whether DDL can run inside a transaction
	fn transactional_ddl (&self) -> bool;

	fn connect (&self, raw_config: &JsonValue) -> Result<Box<dyn SqlSession>, String>;
	fn introspect (&self, session: &mut dyn SqlSession) -> Result<DbSchema, String>;
	fn table_exists (&self, session: &mut dyn SqlSession, table: &str) -> Result<bool, String>;
	fn import_table_options (&self, options: &BTreeMap<String, String>, defaults: &[DialectBlock]) -> Result<BTreeMap<String, String>, String>;

	/// Lock against parallel `apply` with release when the session is closed
	fn lock (&self, session: &mut dyn SqlSession) -> Result<(), String>;

	/// End (exclusive) of the comment that starts at `chars[at]`, if one starts there
	fn comment_end (&self, chars: &[char], at: usize) -> Option<usize>;
	/// End (exclusive) of the quoted string or identifier that starts at `chars[at]`, if one starts there.
	/// `before` - text of the statement up to it.
	fn quoted_end (&self, chars: &[char], at: usize, before: &str) -> Option<usize>;

	fn table_name (&self, table: &Table, _defaults: &[DialectBlock]) -> Result<String, String> {
		return Ok(table.table.clone());
	}

	fn index_name (&self, _table: &Table, index: &Index) -> Result<String, String> {
		return Ok(index.name.clone());
	}

	fn import_index_name (&self, _table: &str, index: &str) -> (String, BTreeMap<String, String>) {
		return (index.to_owned(), BTreeMap::new());
	}

	fn introspect_for (&self, session: &mut dyn SqlSession, _expected: &DbSchema) -> Result<DbSchema, String> {
		return self.introspect(session);
	}

	fn quote (&self, name: &str) -> String {
		return format!("`{}`", name.replace('`', "``"));
	}
}
