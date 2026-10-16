pub mod connection;
pub mod introspect;
pub mod models;
pub mod query;
pub mod query_error;

#[allow(unused_imports)]
pub use connection::{connect, connect_with_url};
#[allow(unused_imports)]
pub use introspect::{
    list_databases, load_extensions, load_foreign_keys, load_functions, load_indexes, load_schemas,
    load_sequences, load_table_details, load_triggers, load_views,
};
#[allow(unused_imports)]
pub use models::{
    ColumnInfo, DatabaseInfo, ExplainResult, ExtensionInfo, ForeignKeyInfo, FunctionInfo,
    IndexInfo, QueryResult, SchemaInfo, SequenceInfo, TableDetails, TableInfo, TriggerInfo,
    ViewInfo,
};
pub use query::{
    create_schema, drop_schema, execute_cell_update, execute_explain, execute_query,
    execute_row_update, is_protected_schema, is_system_schema, is_temp_schema, rename_schema,
    validate_schema_name,
};
pub use query_error::QueryError;
