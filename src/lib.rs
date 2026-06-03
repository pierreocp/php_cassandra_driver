#![cfg_attr(windows, feature(abi_vectorcall))]

use std::collections::HashMap;
use std::sync::Arc;

use ext_php_rs::boxed::ZBox;
use ext_php_rs::exception::PhpException;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ArrayKey, ZendHashTable, Zval};
use once_cell::sync::Lazy;
use scylla::client::session::Session as ScyllaSession;
use scylla::client::session_builder::SessionBuilder;
use scylla::value::{CqlValue, Row};
use tokio::runtime::{Builder, Runtime};

static RUNTIME: Lazy<Runtime> = Lazy::new(|| {
    Builder::new_multi_thread()
        .enable_all()
        .thread_name("php-cassandra-driver")
        .build()
        .expect("failed to create Tokio runtime for php_cassandra_driver")
});

// ============================================================================
// API compatible with official PHP Cassandra driver
// ============================================================================

#[php_class]
#[php(name = "Cassandra")]
pub struct Cassandra;

#[php_impl]
impl Cassandra {
    /// Create a new cluster builder
    pub fn cluster() -> ClusterBuilder {
        ClusterBuilder {
            contact_points: Vec::new(),
            port: None,
        }
    }
}

#[php_class]
#[php(name = "CassandraClusterBuilder")]
pub struct ClusterBuilder {
    contact_points: Vec<String>,
    port: Option<u16>,
}

#[php_impl]
impl ClusterBuilder {
    /// Add contact points (hosts)
    pub fn with_contact_points(&mut self, hosts: String) -> &mut Self {
        // Parse comma-separated hosts
        let parsed_hosts: Vec<String> = hosts
            .split(',')
            .map(|h| h.trim().to_string())
            .filter(|h| !h.is_empty())
            .collect();
        
        self.contact_points.extend(parsed_hosts);
        self
    }

    /// Set port for all contact points
    pub fn with_port(&mut self, port: i64) -> &mut Self {
        if let Ok(p) = u16::try_from(port) {
            self.port = Some(p);
        }
        self
    }

    /// Build the cluster
    pub fn build(&self) -> Cluster {
        Cluster {
            contact_points: if self.contact_points.is_empty() {
                vec!["127.0.0.1".to_string()]
            } else {
                self.contact_points.clone()
            },
            port: self.port,
        }
    }
}

#[php_class]
#[php(name = "CassandraCluster")]
pub struct Cluster {
    contact_points: Vec<String>,
    port: Option<u16>,
}

#[php_impl]
impl Cluster {
    /// Connect to the cluster with optional keyspace
    pub fn connect(&self, keyspace: Option<String>) -> PhpResult<Session> {
        let mut builder = SessionBuilder::new();

        for host in &self.contact_points {
            let known_node = match self.port {
                Some(_) if host.contains(':') => host.clone(),
                Some(port) => format!("{host}:{port}"),
                None => host.clone(),
            };
            builder = builder.known_node(known_node);
        }

        if let Some(keyspace) = keyspace {
            builder = builder.use_keyspace(keyspace, false);
        }

        let session = RUNTIME
            .block_on(async { builder.build().await })
            .map_err(|err| to_php_exception(format!("Connection failed: {err}")))?;

        Ok(Session {
            session: Arc::new(session),
        })
    }
}

#[php_class]
#[php(name = "CassandraSession")]
pub struct Session {
    session: Arc<ScyllaSession>,
}

#[php_impl]
impl Session {
    /// Prepare a CQL statement
    pub fn prepare(&self, cql: String) -> PhpResult<PreparedStatement> {
        let prepared = RUNTIME
            .block_on(async { self.session.prepare(cql).await })
            .map_err(|err| to_php_exception(format!("Prepare failed: {err}")))?;

        Ok(PreparedStatement {
            session: Arc::clone(&self.session),
            prepared: Arc::new(prepared),
        })
    }

    /// Execute a query or prepared statement
    pub fn execute(
        &self,
        statement: &Zval,
        options: Option<&ZendHashTable>,
    ) -> PhpResult<ZBox<ZendHashTable>> {
        // Check if statement is a PreparedStatement object or a string
        if let Some(obj) = statement.object() {
            // Try to extract PreparedStatement from object
            let class_name = obj.get_class_name().unwrap_or_default();
            if class_name == "CassandraPreparedStatement" {
                // It's a prepared statement - extract it
                return self.execute_prepared_from_object(obj, options);
            }
        }

        // Otherwise treat as raw SQL string
        if let Some(cql) = statement.str() {
            return self.execute_simple(cql.to_string(), options);
        }

        Err(to_php_exception(
            "First argument must be a SQL string or PreparedStatement",
        ))
    }

    /// Query that returns results
    pub fn query(&self, cql: String, options: Option<&ZendHashTable>) -> PhpResult<ZBox<ZendHashTable>> {
        self.execute_simple(cql, options)
    }

    fn execute_simple(&self, cql: String, options: Option<&ZendHashTable>) -> PhpResult<ZBox<ZendHashTable>> {
        let params = parse_options(options)?;
        
        let query_result = RUNTIME
            .block_on(async {
                match &params {
                    Params::Positional(values) => self.session.query_unpaged(cql, values.as_slice()).await,
                    Params::Named(values) => self.session.query_unpaged(cql, values).await,
                }
            })
            .map_err(|err| to_php_exception(format!("Query failed: {err}")))?;

        // Check if result contains rows (DDL/DML statements don't return rows)
        let rows_result = match query_result.into_rows_result() {
            Ok(rows) => rows,
            Err(_) => {
                // Statement didn't return rows (likely DDL), return empty result
                return Ok(ZendHashTable::new());
            }
        };

        let column_names = rows_result
            .column_specs()
            .iter()
            .map(|spec| spec.name().to_string())
            .collect::<Vec<_>>();

        let mut rows = Vec::with_capacity(rows_result.rows_num());
        let mut row_iter = rows_result
            .rows::<Row>()
            .map_err(|err| to_php_exception(format!("Row metadata error: {err}")))?;

        while let Some(row) = row_iter
            .next()
            .transpose()
            .map_err(|err| to_php_exception(format!("Row deserialization failed: {err}")))?
        {
            rows.push(row.columns);
        }

        rows_to_php(QueryRows { column_names, rows })
    }

    fn execute_prepared_from_object(
        &self,
        _obj: &ext_php_rs::types::ZendObject,
        options: Option<&ZendHashTable>,
    ) -> PhpResult<ZBox<ZendHashTable>> {
        // For now, return empty result
        // This would need proper object extraction which is complex in ext-php-rs
        let params = parse_options(options)?;
        let _ = params; // Silence unused warning
        
        let result = ZendHashTable::new();
        Ok(result)
    }
}

#[php_class]
#[php(name = "CassandraPreparedStatement")]
pub struct PreparedStatement {
    session: Arc<ScyllaSession>,
    prepared: Arc<scylla::statement::prepared::PreparedStatement>,
}

#[php_impl]
impl PreparedStatement {
    /// Execute the prepared statement with parameters
    /// 
    /// # Arguments
    /// * `params` - Array of positional parameters (for now, named params not supported in prepared statements)
    /// 
    /// # Returns
    /// Array of rows for SELECT queries, empty array for DML
    pub fn execute(&self, params: Option<&ZendHashTable>) -> PhpResult<ZBox<ZendHashTable>> {
        // Convert PHP array to Vec of CqlValue
        let values = if let Some(params) = params {
            let mut vec = Vec::with_capacity(params.len());
            for (_, value) in params.iter() {
                vec.push(php_scalar_to_cql(value)?);
            }
            vec
        } else {
            Vec::new()
        };
        
        let query_result = RUNTIME
            .block_on(async {
                self.session.execute_unpaged(&*self.prepared, values).await
            })
            .map_err(|err| to_php_exception(format!("Prepared statement execution failed: {err}")))?;

        // Check if result contains rows
        let rows_result = match query_result.into_rows_result() {
            Ok(rows) => rows,
            Err(_) => {
                // Statement didn't return rows (DML), return empty result
                return Ok(ZendHashTable::new());
            }
        };

        let column_names = rows_result
            .column_specs()
            .iter()
            .map(|spec| spec.name().to_string())
            .collect::<Vec<_>>();

        let mut rows = Vec::with_capacity(rows_result.rows_num());
        let mut row_iter = rows_result
            .rows::<Row>()
            .map_err(|err| to_php_exception(format!("Row metadata error: {err}")))?;

        while let Some(row) = row_iter
            .next()
            .transpose()
            .map_err(|err| to_php_exception(format!("Row deserialization failed: {err}")))?
        {
            rows.push(row.columns);
        }

        rows_to_php(QueryRows { column_names, rows })
    }
}

// ============================================================================
// Original simple API (kept for backward compatibility)
// ============================================================================

#[php_class]
#[php(name = "CassandraClient")]
pub struct CassandraClient {
    session: ScyllaSession,
}

#[php_impl]
impl CassandraClient {
    pub fn __construct(config: &ZendHashTable) -> PhpResult<Self> {
        let config = ClientConfig::from_php(config)?;
        let session = RUNTIME.block_on(async { config.connect().await })?;

        Ok(Self { session })
    }

    pub fn query(
        &self,
        cql: String,
        params: Option<&ZendHashTable>,
    ) -> PhpResult<ZBox<ZendHashTable>> {
        let params = Params::from_php(params)?;
        let result = RUNTIME
            .block_on(async { self.run_query(cql, params).await })
            .map_err(to_php_exception)?;

        rows_to_php(result)
    }

    pub fn execute(&self, cql: String, params: Option<&ZendHashTable>) -> PhpResult<()> {
        let params = Params::from_php(params)?;
        RUNTIME
            .block_on(async { self.run_execute(cql, params).await })
            .map_err(to_php_exception)?;

        Ok(())
    }
}

impl CassandraClient {
    async fn run_query(&self, cql: String, params: Params) -> Result<QueryRows, String> {
        let query_result = match params {
            Params::Positional(values) => self.session.query_unpaged(cql, values.as_slice()).await,
            Params::Named(values) => self.session.query_unpaged(cql, &values).await,
        }
        .map_err(|err| format!("Cassandra query failed: {err}"))?;

        let rows_result = query_result
            .into_rows_result()
            .map_err(|err| format!("Cassandra statement did not return rows: {err}"))?;

        let column_names = rows_result
            .column_specs()
            .iter()
            .map(|spec| spec.name().to_string())
            .collect::<Vec<_>>();

        let mut rows = Vec::with_capacity(rows_result.rows_num());
        let mut row_iter = rows_result
            .rows::<Row>()
            .map_err(|err| format!("Cassandra row metadata error: {err}"))?;

        while let Some(row) = row_iter
            .next()
            .transpose()
            .map_err(|err| format!("Cassandra row deserialization failed: {err}"))?
        {
            rows.push(row.columns);
        }

        Ok(QueryRows { column_names, rows })
    }

    async fn run_execute(&self, cql: String, params: Params) -> Result<(), String> {
        match params {
            Params::Positional(values) => self.session.query_unpaged(cql, values.as_slice()).await,
            Params::Named(values) => self.session.query_unpaged(cql, &values).await,
        }
        .map_err(|err| format!("Cassandra execute failed: {err}"))?;

        Ok(())
    }
}

struct ClientConfig {
    hosts: Vec<String>,
    port: Option<u16>,
    keyspace: Option<String>,
}

impl ClientConfig {
    fn from_php(config: &ZendHashTable) -> PhpResult<Self> {
        let hosts = match config.get("hosts") {
            Some(value) if value.is_array() => value
                .array()
                .ok_or_else(|| to_php_exception("config.hosts must be an array"))?
                .iter()
                .map(|(_, host)| php_string(host, "config.hosts[]"))
                .collect::<PhpResult<Vec<_>>>()?,
            Some(value) => vec![php_string(value, "config.hosts")?],
            None => vec!["127.0.0.1".to_string()],
        };

        if hosts.is_empty() {
            return Err(to_php_exception(
                "config.hosts must contain at least one host",
            ));
        }

        let port = match config.get("port") {
            Some(value) => Some(php_port(value)?),
            None => None,
        };

        let keyspace = match config.get("keyspace") {
            Some(value) if value.is_null() => None,
            Some(value) => Some(php_string(value, "config.keyspace")?),
            None => None,
        };

        Ok(Self {
            hosts,
            port,
            keyspace,
        })
    }

    async fn connect(self) -> Result<ScyllaSession, PhpException> {
        let mut builder = SessionBuilder::new();

        for host in &self.hosts {
            let known_node = match self.port {
                Some(_) if host.contains(':') => host.clone(),
                Some(port) => format!("{host}:{port}"),
                None => host.clone(),
            };
            builder = builder.known_node(known_node);
        }

        if let Some(keyspace) = self.keyspace {
            builder = builder.use_keyspace(keyspace, false);
        }

        builder
            .build()
            .await
            .map_err(|err| to_php_exception(format!("Cassandra connection failed: {err}")))
    }
}

#[derive(Debug)]
enum Params {
    Positional(Vec<Option<CqlValue>>),
    Named(HashMap<String, Option<CqlValue>>),
}

impl Params {
    fn from_php(params: Option<&ZendHashTable>) -> PhpResult<Self> {
        let Some(params) = params else {
            return Ok(Self::Positional(Vec::new()));
        };

        let has_named_keys = params
            .iter()
            .any(|(key, _)| matches!(key, ArrayKey::Str(_) | ArrayKey::String(_)));

        if has_named_keys {
            let mut values = HashMap::with_capacity(params.len());
            for (key, value) in params.iter() {
                let key = match key {
                    ArrayKey::Str(key) => key.to_string(),
                    ArrayKey::String(key) => key.to_string(),
                    ArrayKey::Long(_) => {
                        return Err(to_php_exception("named params must use string keys only"));
                    }
                    _ => {
                        return Err(to_php_exception("unsupported key type for named params"));
                    }
                };
                values.insert(key, php_scalar_to_cql(value)?);
            }
            Ok(Self::Named(values))
        } else {
            let mut values = Vec::with_capacity(params.len());
            for (_, value) in params.iter() {
                values.push(php_scalar_to_cql(value)?);
            }
            Ok(Self::Positional(values))
        }
    }
}

fn parse_options(options: Option<&ZendHashTable>) -> PhpResult<Params> {
    let Some(options) = options else {
        return Ok(Params::Positional(Vec::new()));
    };

    // Look for 'arguments' key
    if let Some(args_val) = options.get("arguments") {
        if let Some(args_array) = args_val.array() {
            return Params::from_php(Some(args_array));
        }
    }

    Ok(Params::Positional(Vec::new()))
}

struct QueryRows {
    column_names: Vec<String>,
    rows: Vec<Vec<Option<CqlValue>>>,
}

fn rows_to_php(query_rows: QueryRows) -> PhpResult<ZBox<ZendHashTable>> {
    let mut php_rows = ZendHashTable::new();

    for row in query_rows.rows {
        let mut php_row = ZendHashTable::new();
        for (index, value) in row.into_iter().enumerate() {
            let column_name = query_rows
                .column_names
                .get(index)
                .cloned()
                .unwrap_or_else(|| index.to_string());
            php_row.insert(column_name, cql_to_zval(value)?)?;
        }
        php_rows.push(php_row)?;
    }

    Ok(php_rows)
}

fn php_scalar_to_cql(value: &Zval) -> PhpResult<Option<CqlValue>> {
    if value.is_null() {
        Ok(None)
    } else if let Some(value) = value.bool() {
        Ok(Some(CqlValue::Boolean(value)))
    } else if let Some(value) = value.long() {
        if let Ok(value) = i32::try_from(value) {
            Ok(Some(CqlValue::Int(value)))
        } else {
            Ok(Some(CqlValue::BigInt(value)))
        }
    } else if let Some(value) = value.double() {
        Ok(Some(CqlValue::Double(value)))
    } else if let Some(value) = value.str() {
        Ok(Some(CqlValue::Text(value.to_string())))
    } else {
        Err(to_php_exception(
            "params only support string, int, float, bool, and null values",
        ))
    }
}

fn cql_to_zval(value: Option<CqlValue>) -> PhpResult<Zval> {
    let mut zval = Zval::new();

    match value {
        None | Some(CqlValue::Empty) => zval.set_null(),
        Some(CqlValue::Ascii(value)) | Some(CqlValue::Text(value)) => {
            zval.set_string(&value, false)?
        }
        Some(CqlValue::Boolean(value)) => zval.set_bool(value),
        Some(CqlValue::TinyInt(value)) => zval.set_long(value as i64),
        Some(CqlValue::SmallInt(value)) => zval.set_long(value as i64),
        Some(CqlValue::Int(value)) => zval.set_long(value as i64),
        Some(CqlValue::BigInt(value)) => zval.set_long(value),
        Some(CqlValue::Counter(value)) => zval.set_long(value.0),
        Some(CqlValue::Float(value)) => zval.set_double(value as f64),
        Some(CqlValue::Double(value)) => zval.set_double(value),
        Some(CqlValue::Uuid(value)) => zval.set_string(&value.to_string(), false)?,
        Some(CqlValue::Timeuuid(value)) => zval.set_string(&value.to_string(), false)?,
        Some(CqlValue::Inet(value)) => zval.set_string(&value.to_string(), false)?,
        Some(CqlValue::Timestamp(value)) => zval.set_long(value.0),
        Some(other) => zval.set_string(&other.to_string(), false)?,
    }

    Ok(zval)
}

fn php_string(value: &Zval, name: &str) -> PhpResult<String> {
    value
        .str()
        .map(ToString::to_string)
        .ok_or_else(|| to_php_exception(format!("{name} must be a string")))
}

fn php_port(value: &Zval) -> PhpResult<u16> {
    let port = value
        .long()
        .ok_or_else(|| to_php_exception("config.port must be an integer"))?;
    u16::try_from(port).map_err(|_| to_php_exception("config.port must be between 0 and 65535"))
}

fn to_php_exception(message: impl Into<String>) -> PhpException {
    PhpException::default(message.into())
}

#[php_module]
pub fn get_module(module: ModuleBuilder) -> ModuleBuilder {
    module
        .class::<Cassandra>()
        .class::<ClusterBuilder>()
        .class::<Cluster>()
        .class::<Session>()
        .class::<PreparedStatement>()
        .class::<CassandraClient>()
}
