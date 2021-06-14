mod pg_extern;
mod postgres_enum;
mod postgres_hash;
mod postgres_ord;
mod postgres_type;
mod pg_schema;
mod control_file;

pub use pg_extern::{PgExtern, InventoryPgExtern, InventoryPgExternReturn, InventoryPgExternInput, InventoryPgOperator};
pub use postgres_enum::{PostgresEnum, InventoryPostgresEnum};
pub use postgres_hash::{PostgresHash, InventoryPostgresHash};
pub use postgres_ord::{PostgresOrd, InventoryPostgresOrd};
pub use postgres_type::{PostgresType, InventoryPostgresType};
pub use pg_schema::{Schema, InventorySchema};
pub use control_file::{ControlFile, ControlFileError};

// Reexports for the pgx extension inventory builders.
#[doc(hidden)]
pub use inventory;
#[doc(hidden)]
pub use include_dir;

#[derive(Debug, Clone)]
pub struct ExtensionSql {
    pub sql: &'static str,
    pub file: &'static str,
    pub line: u32,
}

use std::collections::HashMap;
use core::any::TypeId;
use crate::ExternArgs;

#[derive(Debug, Clone)]
pub struct PgxSql<'a> {
    pub load_order: HashMap<&'a str, &'a str>,
    pub type_mappings: HashMap<TypeId, String>,
    pub control: ControlFile,
    pub schemas: Vec<&'a InventorySchema>,
    pub extension_sql: Vec<&'a ExtensionSql>,
    pub externs: Vec<&'a InventoryPgExtern>,
    pub types: Vec<&'a InventoryPostgresType>,
    pub enums: Vec<&'a InventoryPostgresEnum>,
    pub ords: Vec<&'a InventoryPostgresOrd>,
    pub hashes: Vec<&'a InventoryPostgresHash>
}

impl<'a> PgxSql<'a> {
    pub fn to_file(&self, file: impl AsRef<str>) -> Result<(), Box<dyn std::error::Error>> {
        use std::{fs::{File, create_dir_all}, path::Path, io::Write};
        let generated = self.to_sql();
        let path = Path::new(file.as_ref());
        let parent = path.parent();
        if let Some(parent) = parent {
            create_dir_all(parent)?;
        }
        let mut out = File::create(path)?;
        write!(out, "{}", generated)?;
        Ok(())
    }

    pub fn schema_alias_of(&self, module_path: &'static str) -> Option<String> {
        let mut needle = None;
        for schema in &self.schemas {
            if schema.module_path.starts_with(module_path) {
                needle = Some(schema.name.to_string());
                break;
            }
        }
        needle
    }

    pub fn schema_prefix_for(&self, module_path: &'static str) -> String {
        self.schema_alias_of(module_path).or_else(|| {
            self.control.schema.clone()
        }).map(|v| (v + ".").to_string()).unwrap_or_else(|| "".to_string())
    }

    pub fn to_sql(&self) -> String {
        format!("\
                -- This file is auto generated by pgx.\n\
                -- `./sql/load-order.txt` items.
                {load_order}\n\
                -- `extension_sql!()` defined SQL.\n\
                {extension_sql}\n\
                -- Schemas defined by `#[pg_schema] mod {{ /* ... */ }}` blocks (except `public` & `pg_catalog`)\n\
                {schemas}\n\
                -- Enums derived via `#[derive(PostgresEnum)]`\n\
                {enums}\n\
                -- Shell types for types defined by `#[derive(PostgresType)]`\n\
                {shell_types}\n\
                -- Functions defined by `#[pg_extern]`\n\
                {externs_with_operators}\n\
                -- Types defined by `#[derive(PostgresType)]`\n\
                {materialized_types}\n\
                -- Operator classes defined by `#[derive(PostgresHash, PostgresOrd)]`\n\
                {operator_classes}\n\
            ",
            load_order = self.load_order_items(),
            extension_sql = self.extension_sql(),
            schemas = self.schemas(),
            enums = self.enums(),
            shell_types = self.shell_types(),
            externs_with_operators = self.externs_with_operators(),
            materialized_types = self.materialized_types(),
            operator_classes = self.operator_classes(),
        )
    }

    fn load_order_items(&self) -> String {
        let mut buf = String::new();
        for (item, sql) in &self.load_order {
            buf.push_str(&format!("\n\
                    -- start of `sql/{item}`\n\
                    {sql}\
                    -- end of `sql/{item}`\n\
                ",
                item = item,
                sql = sql,
            ))
        }
        buf
    }

    fn extension_sql(&self) -> String {
        let mut buf = String::new();
        for item in &self.extension_sql {
            buf.push_str(&format!("\
                                -- {file}:{line}\n\
                                {sql}\
                            ",
                                  file = item.file,
                                  line = item.line,
                                  sql = item.sql,
            ))
        }
        buf
    }

    fn schemas(&self) -> String {
        let mut buf = String::new();
        if let Some(schema) = &self.control.schema {
            buf.push_str(&format!("CREATE SCHEMA IF NOT EXISTS {};\n", schema));
        }
        for item in &self.schemas {
            match item.name {
                "pg_catalog" | "public" =>  (),
                name => buf.push_str(&format!("\
                                    CREATE SCHEMA IF NOT EXISTS {name}; /* {module_path} */\n\
                                ",
                                              name = name,
                                              module_path = item.module_path,
                )),
            };
        }
        buf
    }

    fn enums(&self) -> String {
        let mut buf = String::new();
        for item in &self.enums {
            buf.push_str(&format!("\
                                -- {file}:{line}\n\
                                -- {full_path} - {id:?}\n\
                                CREATE TYPE {schema}{name} AS ENUM (\n\
                                    {variants}\
                                );\n\
                            ",
                                  schema = self.schema_prefix_for(item.module_path),
                                  full_path = item.full_path,
                                  file = item.file,
                                  line = item.line,
                                  id = item.id,
                                  name = item.name,
                                  variants = item.variants.iter().map(|variant| format!("\t'{}',\n", variant)).collect::<String>(),
            ));
        }
        buf
    }

    fn shell_types(&self) -> String {
        let mut buf = String::new();
        for item in &self.types {
            buf.push_str(&format!("\n\
                                -- {file}:{line}\n\
                                -- {full_path}\n\
                                CREATE TYPE {schema}{name};\n\
                            ",
                                  schema = self.schema_prefix_for(item.module_path),
                                  full_path = item.full_path,
                                  file = item.file,
                                  line = item.line,
                                  name = item.name,
            ))
        }
        buf
    }

    fn externs_with_operators(&self) -> String {
        let mut buf = String::new();
        for item in &self.externs {
            let mut extern_attrs = item.extern_attrs.clone();
            let mut strict_upgrade = true;
            if !extern_attrs.iter().any(|i| i == &ExternArgs::Strict) {
                for arg in &item.fn_args {
                    if arg.is_optional {
                        strict_upgrade = false;
                    }
                }
            }
            if strict_upgrade {
                extern_attrs.push(ExternArgs::Strict);
            }

            let fn_sql = format!("\
                                CREATE OR REPLACE FUNCTION {schema}\"{name}\"({arguments}) {returns}\n\
                                {extern_attrs}\
                                {search_path}\
                                LANGUAGE c /* Rust */\n\
                                AS 'MODULE_PATHNAME', '{name}_wrapper';\
                            ",
                                 schema = self.schema_prefix_for(item.module_path),
                                 name = item.name,
                                 arguments = if !item.fn_args.is_empty() {
                                     String::from("\n") + &item.fn_args.iter().enumerate().map(|(idx, arg)| {
                                         let needs_comma = idx < (item.fn_args.len() - 1);
                                         format!("\
                                            \t\"{pattern}\" {sql_type} {default}{maybe_comma}/* {ty_name} */\
                                        ",
                                                 pattern = arg.pattern,
                                                 sql_type = self.type_id_to_sql_type(arg.ty_id).unwrap_or_else(|| arg.ty_name.to_string()),
                                                 default = if let Some(def) = arg.default { format!("DEFAULT {} ", def) } else { String::from("") },
                                                 maybe_comma = if needs_comma { "," } else { "" },
                                                 ty_name = arg.ty_name,
                                         )
                                     }).collect::<Vec<_>>().join("\n") + "\n"
                                 } else { Default::default() },
                                 returns = match &item.fn_return {
                                     InventoryPgExternReturn::None => String::from("RETURNS void"),
                                     InventoryPgExternReturn::Type { id, name } => format!("RETURNS {} /* {} */", self.type_id_to_sql_type(*id).unwrap_or_else(|| name.to_string()), name),
                                     InventoryPgExternReturn::SetOf { id, name } => format!("RETURNS SETOF {} /* {} */", self.type_id_to_sql_type(*id).unwrap_or_else(|| name.to_string()), name),
                                     InventoryPgExternReturn::Iterated(vec) => format!("RETURNS TABLE ({}\n)",
                                                                                                                vec.iter().map(|(id, ty_name, col_name)| format!("\n\t\"{}\" {} /* {} */", col_name.unwrap(), self.type_id_to_sql_type(*id).unwrap_or_else(|| ty_name.to_string()), ty_name)).collect::<Vec<_>>().join(",")
                                     ),
                                 },
                                 search_path = if let Some(search_path) = &item.search_path {
                                     let retval = format!("SET search_path TO {}", search_path.join(", "));
                                     retval + "\n"
                                 } else { Default::default() },
                                 extern_attrs = if extern_attrs.is_empty() {
                                     String::default()
                                 } else {
                                     let mut retval = extern_attrs.iter().map(|attr| format!("{}", attr).to_uppercase()).collect::<Vec<_>>().join(" ");
                                     retval.push('\n');
                                     retval
                                 },
            );

            let ext_sql = format!("\n\
                                -- {file}:{line}\n\
                                -- {module_path}::{name}\n\
                                {fn_sql}\n\
                                {overridden}\
                            ",
                                  name = item.name,
                                  module_path = item.module_path,
                                  file = item.file,
                                  line = item.line,
                                  fn_sql = if item.overridden.is_some() {
                                      let mut inner = fn_sql.lines().map(|f| format!("-- {}", f)).collect::<Vec<_>>().join("\n");
                                      inner.push_str("\n--\n-- Overridden as (due to a `//` comment with a `sql` code block):");
                                      inner
                                  } else {
                                      fn_sql
                                  },
                                  overridden = item.overridden.map(|f| f.to_owned() + "\n").unwrap_or_default(),
            );

            let rendered = match (item.overridden, &item.operator) {
                (None, Some(op)) => {
                    let mut optionals = vec![];
                    if let Some(it) = op.commutator {
                        optionals.push(format!("\tCOMMUTATOR = {}", it));
                    };
                    if let Some(it) = op.negator {
                        optionals.push(format!("\tNEGATOR = {}", it));
                    };
                    if let Some(it) = op.restrict {
                        optionals.push(format!("\tRESTRICT = {}", it));
                    };
                    if let Some(it) = op.join {
                        optionals.push(format!("\tJOIN = {}", it));
                    };
                    if op.hashes {
                        optionals.push(String::from("\tHASHES"));
                    };
                    if op.merges {
                        optionals.push(String::from("\tMERGES"));
                    };
                    let operator_sql = format!("\n\
                                        -- {file}:{line}\n\
                                        -- {module_path}::{name}\n\
                                        CREATE OPERATOR {opname} (\n\
                                            \tPROCEDURE=\"{name}\",\n\
                                            \tLEFTARG={left_arg}, /* {left_name} */\n\
                                            \tRIGHTARG={right_arg}, /* {right_name} */\n\
                                            {optionals}\
                                        );
                                    ",
                                               opname = op.opname.unwrap(),
                                               file = item.file,
                                               line = item.line,
                                               name = item.name,
                                               module_path = item.module_path,
                                               left_name = item.fn_args.get(0).unwrap().ty_name,
                                               right_name = item.fn_args.get(1).unwrap().ty_name,
                                               left_arg = self.type_id_to_sql_type(item.fn_args.get(0).unwrap().ty_id).unwrap_or_else(|| item.fn_args.get(0).unwrap().ty_name.to_string()),
                                               right_arg = self.type_id_to_sql_type(item.fn_args.get(1).unwrap().ty_id).unwrap_or_else(|| item.fn_args.get(1).unwrap().ty_name.to_string()),
                                               optionals = optionals.join(",\n") + "\n"
                    );
                    ext_sql + &operator_sql
                },
                (None, None) | (Some(_), Some(_)) | (Some(_), None) => ext_sql,
            };
            buf.push_str(&rendered)
        }
        buf
    }

    fn materialized_types(&self) -> String {
        let mut buf = String::new();
        for item in &self.types {
            buf.push_str(&format!("\n\
                                -- {file}:{line}\n\
                                -- {full_path} - {id:?}\n\
                                CREATE TYPE {schema}{name} (\n\
                                    \tINTERNALLENGTH = variable,\n\
                                    \tINPUT = {in_fn},\n\
                                    \tOUTPUT = {out_fn},\n\
                                    \tSTORAGE = extended\n\
                                );
                            ",
                                  full_path = item.full_path,
                                  file = item.file,
                                  line = item.line,
                                  schema = self.schema_prefix_for(item.module_path),
                                  id = item.id,
                                  name = item.name,
                                  in_fn = item.in_fn,
                                  out_fn = item.out_fn,
            ));
        }
        buf
    }

    fn operator_classes(&self) -> String {
        let mut buf = String::new();
        for item in &self.hashes {
            buf.push_str(&format!("\n\
                            -- {file}:{line}\n\
                            -- {full_path}\n\
                            -- {id:?}\n\
                            CREATE OPERATOR FAMILY {name}_hash_ops USING hash;\n\
                            CREATE OPERATOR CLASS {name}_hash_ops DEFAULT FOR TYPE {name} USING hash FAMILY {name}_hash_ops AS\n\
                                \tOPERATOR    1   =  ({name}, {name}),\n\
                                \tFUNCTION    1   {name}_hash({name});\
                            ",
                                  name = item.name,
                                  full_path = item.full_path,
                                  file = item.file,
                                  line = item.line,
                                  id = item.id,
            ));
        }
        for item in &self.ords {
            buf.push_str(&format!("\n\
                            -- {file}:{line}\n\
                            -- {full_path}\n\
                            -- {id:?}\n\
                            CREATE OPERATOR FAMILY {name}_btree_ops USING btree;\n\
                            CREATE OPERATOR CLASS {name}_btree_ops DEFAULT FOR TYPE {name} USING btree FAMILY {name}_btree_ops AS\n\
                                  \tOPERATOR 1 < ,\n\
                                  \tOPERATOR 2 <= ,\n\
                                  \tOPERATOR 3 = ,\n\
                                  \tOPERATOR 4 >= ,\n\
                                  \tOPERATOR 5 > ,\n\
                                  \tFUNCTION 1 {name}_cmp({name}, {name});\n\
                            ",
                                  name = item.name,
                                  full_path = item.full_path,
                                  file = item.file,
                                  line = item.line,
                                  id = item.id,
            ))
        }
        buf
    }

    pub fn register_types(&mut self) {
        for item in self.enums.clone() {
            self.map_type_id_to_sql_type(item.id, item.name);
            self.map_type_id_to_sql_type(item.option_id, item.name);
            self.map_type_id_to_sql_type(item.vec_id, format!("{}[]", item.name));
        }
        for item in self.types.clone() {
            self.map_type_id_to_sql_type(item.id, item.name);
            self.map_type_id_to_sql_type(item.option_id, item.name);
            self.map_type_id_to_sql_type(item.vec_id, format!("{}[]", item.name));
        }
    }

    pub fn type_id_to_sql_type(&self, id: TypeId) -> Option<String> {
        self.type_mappings
            .get(&id)
            .map(|f| f.clone())
    }
    pub fn map_type_to_sql_type<T: 'static>(&mut self, sql: impl AsRef<str>) {
        let sql = sql.as_ref().to_string();
        self.type_mappings
            .insert(TypeId::of::<T>(), sql.clone());
        self.type_mappings
            .insert(TypeId::of::<Option<T>>(), sql.clone());
        self.type_mappings
            .insert(TypeId::of::<Vec<T>>(), format!("{}[]", sql));
    }

    pub fn map_type_id_to_sql_type(&mut self, id: TypeId, sql: impl AsRef<str>) {
        let sql = sql.as_ref().to_string();
        self.type_mappings.insert(id, sql);
    }

}