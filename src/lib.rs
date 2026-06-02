#![cfg_attr(windows, feature(abi_vectorcall))]

use std::collections::HashMap;

use ext_php_rs::boxed::ZBox;
use ext_php_rs::exception::PhpException;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ArrayKey, ZendHashTable, Zval};
use once_cell::sync::Lazy;
use scylla::client::session::Session;
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

#[php_class]
#[php(name = "CassandraClient")]
pub struct CassandraClient {
    session: Session,
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
            .map(|spec| spec.name.to_string())
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

    async fn connect(self) -> PhpResult<Session> {
        let mut builder = SessionBuilder::new();

        for host in self.hosts {
            let known_node = match self.port {
                Some(_) if host.contains(':') => host,
                Some(port) => format!("{host}:{port}"),
                None => host,
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
            zval.set_string(value, false)?
        }
        Some(CqlValue::Boolean(value)) => zval.set_bool(value),
        Some(CqlValue::TinyInt(value)) => zval.set_long(value.into()),
        Some(CqlValue::SmallInt(value)) => zval.set_long(value.into()),
        Some(CqlValue::Int(value)) => zval.set_long(value.into()),
        Some(CqlValue::BigInt(value)) => zval.set_long(value),
        Some(CqlValue::Counter(value)) => zval.set_long(value.0),
        Some(CqlValue::Float(value)) => zval.set_double(value.into()),
        Some(CqlValue::Double(value)) => zval.set_double(value),
        Some(CqlValue::Uuid(value)) => zval.set_string(value.to_string(), false)?,
        Some(CqlValue::Timeuuid(value)) => zval.set_string(value.to_string(), false)?,
        Some(CqlValue::Inet(value)) => zval.set_string(value.to_string(), false)?,
        Some(CqlValue::Timestamp(value)) => zval.set_long(value.0),
        Some(other) => zval.set_string(other.to_string(), false)?,
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
    module.class::<CassandraClient>()
}
