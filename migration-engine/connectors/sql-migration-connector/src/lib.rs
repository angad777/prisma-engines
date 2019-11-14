#[macro_use]
extern crate log;

mod error;
mod sql_database_migration_inferrer;
mod sql_database_step_applier;
mod sql_destructive_changes_checker;
mod sql_migration;
mod sql_migration_persistence;
mod sql_renderer;
mod sql_schema_calculator;
mod sql_schema_differ;

pub use error::*;
pub use sql_migration::*;

use migration_connector::*;
use quaint::prelude::{ConnectionInfo, SqlFamily};
use sql_connection::{GenericSqlConnection, SyncSqlConnection};
use sql_database_migration_inferrer::*;
use sql_database_step_applier::*;
use sql_destructive_changes_checker::*;
use sql_migration_persistence::*;
use sql_schema_describer::SqlSchemaDescriberBackend;
use std::{fs, path::PathBuf, sync::Arc};

pub type Result<T> = std::result::Result<T, SqlError>;

#[allow(unused, dead_code)]
pub struct SqlMigrationConnector {
    pub connection_info: ConnectionInfo,
    pub schema_name: String,
    pub database: Arc<dyn SyncSqlConnection + Send + Sync + 'static>,
    pub migration_persistence: Arc<dyn MigrationPersistence>,
    pub database_migration_inferrer: Arc<dyn DatabaseMigrationInferrer<SqlMigration>>,
    pub database_migration_step_applier: Arc<dyn DatabaseMigrationStepApplier<SqlMigration>>,
    pub destructive_changes_checker: Arc<dyn DestructiveChangesChecker<SqlMigration>>,
    pub database_introspector: Arc<dyn SqlSchemaDescriberBackend + Send + Sync + 'static>,
}

impl SqlMigrationConnector {
    pub fn new_from_database_str(database_str: &str) -> std::result::Result<Self, ConnectorError> {
        let connection_info =
            ConnectionInfo::from_url(database_str).map_err(|_err| ConnectorError::InvalidDatabaseUrl)?;

        let connection = GenericSqlConnection::from_database_str(database_str, Some("lift"))
            .map_err(SqlError::from)
            .map_err(|err| err.into_connector_error(&connection_info))?;

        Self::create_connector(connection)
    }

    pub fn new(datasource: &dyn datamodel::Source) -> std::result::Result<Self, ConnectorError> {
        let connection_info =
            ConnectionInfo::from_url(&datasource.url().value).map_err(|_err| ConnectorError::InvalidDatabaseUrl)?;

        let connection = GenericSqlConnection::from_datasource(datasource, Some("lift"))
            .map_err(SqlError::from)
            .map_err(|err| err.into_connector_error(&connection_info))?;

        Self::create_connector(connection)
    }

    fn create_connector(connection: GenericSqlConnection) -> std::result::Result<Self, ConnectorError> {
        // async connections can be lazy, so we issue a simple query to fail early if the database
        // is not reachable.
        connection
            .query_raw("SELECT 1", &[])
            .map_err(SqlError::from)
            .map_err(|err| err.into_connector_error(&connection.connection_info()))?;

        let schema_name = connection
            .connection_info()
            .schema_name()
            .unwrap_or_else(|| "lift".to_owned());
        let sql_family = connection.connection_info().sql_family();
        let connection_info = connection.connection_info().clone();

        let conn = Arc::new(connection) as Arc<dyn SyncSqlConnection + Send + Sync>;

        let inspector: Arc<dyn SqlSchemaDescriberBackend + Send + Sync + 'static> = match sql_family {
            SqlFamily::Mysql => Arc::new(sql_schema_describer::mysql::SqlSchemaDescriber::new(Arc::clone(&conn))),
            SqlFamily::Postgres => Arc::new(sql_schema_describer::postgres::SqlSchemaDescriber::new(Arc::clone(
                &conn,
            ))),
            SqlFamily::Sqlite => Arc::new(sql_schema_describer::sqlite::SqlSchemaDescriber::new(Arc::clone(&conn))),
        };

        let migration_persistence = Arc::new(SqlMigrationPersistence {
            connection_info: connection_info.clone(),
            connection: Arc::clone(&conn),
            schema_name: schema_name.clone(),
        });

        let database_migration_inferrer = Arc::new(SqlDatabaseMigrationInferrer {
            connection_info: connection_info.clone(),
            introspector: Arc::clone(&inspector),
            schema_name: schema_name.to_string(),
        });

        let database_migration_step_applier = Arc::new(SqlDatabaseStepApplier {
            connection_info: connection_info.clone(),
            schema_name: schema_name.clone(),
            conn: Arc::clone(&conn),
        });

        let destructive_changes_checker = Arc::new(SqlDestructiveChangesChecker {
            connection_info: connection_info.clone(),
            schema_name: schema_name.clone(),
            database: Arc::clone(&conn),
        });

        Ok(Self {
            connection_info,
            schema_name,
            database: Arc::clone(&conn),
            migration_persistence,
            database_migration_inferrer,
            database_migration_step_applier,
            destructive_changes_checker,
            database_introspector: Arc::clone(&inspector),
        })
    }

    fn create_database_impl(&self, db_name: &str) -> SqlResult<()> {
        match self.connection_info.sql_family() {
            SqlFamily::Postgres => {
                self.database
                    .query_raw(&format!("CREATE DATABASE \"{}\"", db_name), &[])?;

                Ok(())
            }
            SqlFamily::Sqlite => Ok(()),
            SqlFamily::Mysql => {
                self.database
                    .query_raw(&format!("CREATE DATABASE `{}`", db_name), &[])?;

                Ok(())
            }
        }
    }

    fn initialize_impl(&self) -> SqlResult<()> {
        // TODO: this code probably does not ever do anything. The schema/db creation happens already in the helper functions above.
        match &self.connection_info {
            ConnectionInfo::Sqlite { file_path, .. } => {
                let path_buf = PathBuf::from(&file_path);
                match path_buf.parent() {
                    Some(parent_directory) => {
                        fs::create_dir_all(parent_directory).expect("creating the database folders failed")
                    }
                    None => {}
                }
            }
            ConnectionInfo::Postgres(_) => {
                let schema_sql = format!("CREATE SCHEMA IF NOT EXISTS \"{}\";", &self.schema_name);

                debug!("{}", schema_sql);

                self.database.query_raw(&schema_sql, &[])?;
            }
            ConnectionInfo::Mysql(_) => {
                let schema_sql = format!(
                    "CREATE SCHEMA IF NOT EXISTS `{}` DEFAULT CHARACTER SET latin1;",
                    &self.schema_name
                );

                debug!("{}", schema_sql);

                self.database.query_raw(&schema_sql, &[])?;
            }
        }

        self.migration_persistence.init();

        Ok(())
    }
}

impl MigrationConnector for SqlMigrationConnector {
    type DatabaseMigration = SqlMigration;

    fn connector_type(&self) -> &'static str {
        self.connection_info.sql_family().as_str()
    }

    fn create_database(&self, db_name: &str) -> ConnectorResult<()> {
        self.create_database_impl(db_name)
            .map_err(|sql_error| sql_error.into_connector_error(&self.connection_info))
    }

    fn initialize(&self) -> ConnectorResult<()> {
        self.initialize_impl()
            .map_err(|sql_error| sql_error.into_connector_error(&self.connection_info))
    }

    fn reset(&self) -> ConnectorResult<()> {
        self.migration_persistence.reset();
        Ok(())
    }

    fn migration_persistence(&self) -> Arc<dyn MigrationPersistence> {
        Arc::clone(&self.migration_persistence)
    }

    fn database_migration_inferrer(&self) -> Arc<dyn DatabaseMigrationInferrer<SqlMigration>> {
        Arc::clone(&self.database_migration_inferrer)
    }

    fn database_migration_step_applier(&self) -> Arc<dyn DatabaseMigrationStepApplier<SqlMigration>> {
        Arc::clone(&self.database_migration_step_applier)
    }

    fn destructive_changes_checker(&self) -> Arc<dyn DestructiveChangesChecker<SqlMigration>> {
        Arc::clone(&self.destructive_changes_checker)
    }

    fn deserialize_database_migration(&self, json: serde_json::Value) -> SqlMigration {
        serde_json::from_value(json).expect("Deserializing the database migration failed.")
    }
}
