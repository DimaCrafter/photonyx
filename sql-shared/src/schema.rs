use std::{collections::BTreeMap, fmt};


#[derive(Debug, Clone, PartialEq, Default)]
pub struct DbSchema {
	pub tables: BTreeMap<String, TableSchema>,
	/// Named enum type --> Values. Empty for dialects with inline enums like MySQL.
	pub enum_types: BTreeMap<String, Vec<String>>
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableSchema {
	pub name: String,
	pub model: String,
	pub columns: Vec<ColumnSchema>,
	pub primary: Vec<String>,
	pub indexes: Vec<IndexSchema>,
	pub foreign_keys: Vec<ForeignKey>,
	/// Dialect table options
	pub options: BTreeMap<String, String>,
	pub deleted: bool
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnSchema {
	pub name: String,
	pub db_type: ColumnType,
	pub nullable: bool,
	pub default: Option<DefaultValue>,
	/// `ON UPDATE CURRENT_TIMESTAMP` (MySQL), trigger `photonyx_update_now` (PostgreSQL)
	pub on_update_now: bool,
	/// Generated column: the value is computed by the database, it has no default
	pub generated: Option<Generated>,
	pub auto_increment: bool,
	pub collation: Option<String>
}

impl ColumnSchema {
	pub fn new (name: &str, db_type: ColumnType, nullable: bool) -> Self {
		return ColumnSchema {
			name: name.to_owned(),
			db_type,
			nullable,
			default: None,
			on_update_now: false,
			generated: None,
			auto_increment: false,
			collation: None
		};
	}
}

/// `GENERATED ALWAYS AS (expr) STORED|VIRTUAL`
#[derive(Debug, Clone, PartialEq)]
pub struct Generated {
	pub expr: String,
	pub stored: bool
}

#[derive(Debug, Clone, PartialEq)]
pub enum ColumnType {
	Int { size: IntSize, unsigned: bool },
	Float,
	Double,
	Bool,
	Varchar(u32),
	Char(u32),
	Text,
	DateTime,
	Json,
	/// Native enum
	Enum(Vec<String>),
	/// Column of a named enum type (see `DbSchema.enum_types`)
	EnumType(String),
	Other(String)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IntSize {
	Tiny,
	Small,
	Medium,
	Normal,
	Big
}

#[derive(Debug, Clone, PartialEq)]
pub enum DefaultValue {
	Int(i64),
	Float(f64),
	Bool(bool),
	Str(String),
	/// `CURRENT_TIMESTAMP`
	Now,
	Expr(String)
}

#[derive(Debug, Clone, PartialEq)]
pub struct IndexSchema {
	pub name: String,
	pub columns: Vec<String>,
	pub unique: bool,
	/// Index method of the dialect (`btree`, `hash`, `gin`)
	pub using: Option<String>
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForeignKey {
	pub name: String,
	/// Name of the index on the source columns
	pub index: String,
	pub columns: Vec<String>,
	pub ref_table: String,
	pub ref_columns: Vec<String>,
	/// Unique index of the target. Use `None` of a primary key.
	pub ref_index: Option<String>,
	pub on_delete: OnDelete
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnDelete {
	Restrict,
	Cascade,
	SetNull,
	NoAction
}


impl TableSchema {
	pub fn new (name: &str) -> Self {
		TableSchema {
			name: name.to_owned(),
			model: String::new(),
			columns: Vec::new(),
			primary: Vec::new(),
			indexes: Vec::new(),
			foreign_keys: Vec::new(),
			options: BTreeMap::new(),
			deleted: false
		}
	}

	pub fn find_column (&self, name: &str) -> Option<&ColumnSchema> {
		return self.columns.iter().find(|column| column.name == name);
	}

	pub fn find_index (&self, name: &str) -> Option<&IndexSchema> {
		return self.indexes.iter().find(|index| index.name == name);
	}

	pub fn find_foreign_key (&self, name: &str) -> Option<&ForeignKey> {
		return self.foreign_keys.iter().find(|fk| fk.name == name);
	}
}

impl ForeignKey {
	pub fn same (&self, other: &ForeignKey) -> bool {
		return self.columns == other.columns
			&& self.ref_table == other.ref_table
			&& self.ref_columns == other.ref_columns
			&& self.on_delete == other.on_delete;
	}
}

impl ColumnType {
	pub fn describe (&self) -> String {
		return match self {
			ColumnType::Int { size, unsigned } => {
				let name = match size {
					IntSize::Tiny => "tinyint",
					IntSize::Small => "smallint",
					IntSize::Medium => "mediumint",
					IntSize::Normal => "int",
					IntSize::Big => "bigint"
				};

				if *unsigned { format!("{name} unsigned") } else { name.to_owned() }
			}
			ColumnType::Float => "float".to_owned(),
			ColumnType::Double => "double".to_owned(),
			ColumnType::Bool => "bool".to_owned(),
			ColumnType::Varchar(size) => format!("varchar({size})"),
			ColumnType::Char(size) => format!("char({size})"),
			ColumnType::Text => "text".to_owned(),
			ColumnType::DateTime => "datetime".to_owned(),
			ColumnType::Json => "json".to_owned(),
			ColumnType::Enum(values) => format!("enum({})", values.join(",")),
			ColumnType::EnumType(name) => format!("enum type {name}"),
			ColumnType::Other(raw) => raw.clone()
		};
	}
}

impl fmt::Display for ColumnType {
	fn fmt (&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		return f.write_str(&self.describe());
	}
}
