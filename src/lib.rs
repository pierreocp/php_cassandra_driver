#![cfg_attr(windows, feature(abi_vectorcall))]

//! Driver Cassandra/ScyllaDB pour PHP.
//!
//! Expose une API identique au driver officiel datastax/php-driver, telle
//! qu'utilisée par `OCP\Lib\Storage\PdoCassandra` :
//!
//!     $cluster    = Cassandra::cluster()->withContactPoints($hosts)->build();
//!     $session    = $cluster->connect($keyspace);
//!     $stmt       = $session->prepare("SELECT ... WHERE id = :id");
//!     $result     = $session->execute($stmt, ['arguments' => [$id]]);
//!     foreach ($result as $row) { $row['data']; }
//!
//! Aucune modification du code PHP n'est nécessaire.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ext_php_rs::boxed::ZBox;
use ext_php_rs::convert::FromZval;
use ext_php_rs::exception::PhpException;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendHashTable, Zval};
use once_cell::sync::Lazy;
use scylla::client::session::Session as ScyllaSession;
use scylla::client::session_builder::SessionBuilder;
use scylla::statement::prepared::PreparedStatement as ScyllaPrepared;
use scylla::value::{CqlValue, Row};
use tokio::runtime::{Builder, Runtime};

/// Runtime Tokio partagé : le driver scylla est async, on bloque dessus
/// pour exposer une API synchrone à PHP.
static RUNTIME: Lazy<Runtime> = Lazy::new(|| {
    Builder::new_multi_thread()
        .enable_all()
        .thread_name("php-cassandra-driver")
        .build()
        .expect("failed to create Tokio runtime for php_cassandra_driver")
});

// ============================================================================
// Cassandra::cluster()
// ============================================================================

#[php_class]
#[php(name = "Cassandra")]
pub struct Cassandra;

#[php_impl]
impl Cassandra {
    /// `Cassandra::cluster()`
    pub fn cluster() -> ClusterBuilder {
        ClusterBuilder {
            contact_points: Vec::new(),
            port: None,
        }
    }
}

// ============================================================================
// Cluster builder (chaînage ->withContactPoints()->withPort()->build())
// ============================================================================

#[php_class]
#[php(name = "CassandraClusterBuilder")]
pub struct ClusterBuilder {
    contact_points: Vec<String>,
    port: Option<u16>,
}

#[php_impl]
impl ClusterBuilder {
    /// `->withContactPoints("h1,h2,...")` (camelCase appliqué par ext-php-rs).
    pub fn with_contact_points(&mut self, hosts: String) -> &mut Self {
        let parsed: Vec<String> = hosts
            .split(',')
            .map(|h| h.trim().to_string())
            .filter(|h| !h.is_empty())
            .collect();
        self.contact_points.extend(parsed);
        self
    }

    /// `->withPort(9042)`
    pub fn with_port(&mut self, port: i64) -> &mut Self {
        if let Ok(p) = u16::try_from(port) {
            self.port = Some(p);
        }
        self
    }

    /// `->build()`
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

// ============================================================================
// Cluster::connect()
// ============================================================================

#[php_class]
#[php(name = "CassandraCluster")]
pub struct Cluster {
    contact_points: Vec<String>,
    port: Option<u16>,
}

#[php_impl]
impl Cluster {
    /// `$cluster->connect($keyspace)`.
    /// Le port natif CQL par défaut est 9042 (et non 9160, l'ancien port Thrift).
    pub fn connect(&self, keyspace: Option<String>) -> PhpResult<Session> {
        let mut builder = SessionBuilder::new();

        for host in &self.contact_points {
            let node = match self.port {
                Some(_) if host.contains(':') => host.clone(),
                Some(port) => format!("{host}:{port}"),
                None => host.clone(),
            };
            builder = builder.known_node(node);
        }

        if let Some(ks) = keyspace {
            builder = builder.use_keyspace(ks, false);
        }

        let session = RUNTIME
            .block_on(async { builder.build().await })
            .map_err(|err| to_php_exception(format!("Connection failed: {err}")))?;

        Ok(Session {
            session: Arc::new(session),
            cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }
}

// ============================================================================
// Session : prepare() / execute() / query()
// ============================================================================

#[php_class]
#[php(name = "CassandraSession")]
pub struct Session {
    session: Arc<ScyllaSession>,
    /// Cache des statements préparés (clé = CQL) pour éviter un round-trip
    /// PREPARE à chaque appel.
    cache: Arc<Mutex<HashMap<String, Arc<ScyllaPrepared>>>>,
}

#[php_impl]
impl Session {
    /// `$session->prepare($cql)`
    pub fn prepare(&self, cql: String) -> PhpResult<PreparedStatement> {
        if let Some(prepared) = self.cache.lock().unwrap().get(&cql).cloned() {
            return Ok(PreparedStatement {
                session: Arc::clone(&self.session),
                prepared,
            });
        }

        let prepared = RUNTIME
            .block_on(async { self.session.prepare(cql.clone()).await })
            .map_err(|err| to_php_exception(format!("Prepare failed: {err}")))?;
        let prepared = Arc::new(prepared);

        self.cache
            .lock()
            .unwrap()
            .insert(cql, Arc::clone(&prepared));

        Ok(PreparedStatement {
            session: Arc::clone(&self.session),
            prepared,
        })
    }

    /// `$session->execute($stmt, ['arguments' => [...]])`
    ///
    /// `$stmt` peut être un PreparedStatement (cas normal) ou une chaîne CQL
    /// brute (DDL : CREATE/DROP, ou BEGIN BATCH ... APPLY BATCH).
    pub fn execute(
        &self,
        statement: &Zval,
        options: Option<&ZendHashTable>,
    ) -> PhpResult<ZBox<ZendHashTable>> {
        let args = parse_arguments(options)?;

        // Statement préparé : on le ré-extrait de l'objet PHP et on l'exécute.
        if let Some(prepared) = <&PreparedStatement as FromZval>::from_zval(statement) {
            return prepared.run(&args);
        }

        // Sinon, CQL brut (DDL / batch).
        if let Some(cql) = statement.str() {
            return run_query(&self.session, QueryKind::Simple(cql.to_string()), &args);
        }

        Err(to_php_exception(
            "execute() attend un CassandraPreparedStatement ou une chaîne CQL",
        ))
    }

    /// `$session->query($cql, ['arguments' => [...]])`
    pub fn query(
        &self,
        cql: String,
        options: Option<&ZendHashTable>,
    ) -> PhpResult<ZBox<ZendHashTable>> {
        let args = parse_arguments(options)?;
        run_query(&self.session, QueryKind::Simple(cql), &args)
    }
}

// ============================================================================
// PreparedStatement
// ============================================================================

#[php_class]
#[php(name = "CassandraPreparedStatement")]
pub struct PreparedStatement {
    session: Arc<ScyllaSession>,
    prepared: Arc<ScyllaPrepared>,
}

#[php_impl]
impl PreparedStatement {
    /// Variante ISO : `$stmt->execute([...])` où le tableau est la liste
    /// positionnelle d'arguments (sans la clé 'arguments'). Non utilisé par
    /// PdoCassandra mais fourni pour compatibilité.
    pub fn execute(&self, params: Option<&ZendHashTable>) -> PhpResult<ZBox<ZendHashTable>> {
        let args = match params {
            Some(arr) => {
                let mut v = Vec::with_capacity(arr.len());
                for (_, value) in arr.iter() {
                    v.push(php_scalar_to_cql(value)?);
                }
                v
            }
            None => Vec::new(),
        };
        self.run(&args)
    }
}

// Helper interne (non exposé à PHP).
impl PreparedStatement {
    fn run(&self, args: &[Option<CqlValue>]) -> PhpResult<ZBox<ZendHashTable>> {
        run_query(
            &self.session,
            QueryKind::Prepared(Arc::clone(&self.prepared)),
            args,
        )
    }
}

// ============================================================================
// Exécution + conversion des résultats
// ============================================================================

enum QueryKind {
    Simple(String),
    Prepared(Arc<ScyllaPrepared>),
}

/// Exécute une requête (simple ou préparée) et convertit le résultat en
/// tableau PHP : liste de lignes, chaque ligne étant un tableau associatif
/// `colonne => valeur`. Les statements sans lignes (DDL/DML) renvoient `[]`.
fn run_query(
    session: &ScyllaSession,
    kind: QueryKind,
    args: &[Option<CqlValue>],
) -> PhpResult<ZBox<ZendHashTable>> {
    let result = RUNTIME
        .block_on(async {
            match kind {
                QueryKind::Simple(cql) => session.query_unpaged(cql, args).await,
                QueryKind::Prepared(prep) => session.execute_unpaged(&*prep, args).await,
            }
        })
        .map_err(|err| to_php_exception(format!("Query failed: {err}")))?;

    // Les requêtes non-SELECT (INSERT/UPDATE/DELETE/DDL) ne renvoient pas de
    // lignes. Scylla 1.6 peut retourner soit Err (résultat void), soit Ok avec
    // 0 colonnes (résultat vide). Dans les deux cas on renvoie [true] pour que
    // !empty($result) soit true côté PHP, comme l'objet Rows du driver officiel.
    let rows_result = match result.into_rows_result() {
        Ok(rows) => rows,
        Err(_) => return dml_ack(),
    };

    // Résultat void renvoyé comme rows vides par scylla 1.6 (DML/DDL).
    if rows_result.column_specs().len() == 0 {
        return dml_ack();
    }

    let column_names: Vec<String> = rows_result
        .column_specs()
        .iter()
        .map(|spec| spec.name().to_string())
        .collect();

    let mut table = ZendHashTable::new();
    let mut iter = rows_result
        .rows::<Row>()
        .map_err(|err| to_php_exception(format!("Row metadata error: {err}")))?;

    while let Some(row) = iter
        .next()
        .transpose()
        .map_err(|err| to_php_exception(format!("Row deserialization failed: {err}")))?
    {
        let mut php_row = ZendHashTable::new();
        for (index, value) in row.columns.into_iter().enumerate() {
            let name = column_names
                .get(index)
                .cloned()
                .unwrap_or_else(|| index.to_string());
            php_row.insert(name, cql_to_zval(value)?)?;
        }
        table.push(php_row)?;
    }

    Ok(table)
}

/// Retourne `[true]` pour signaler un DML/DDL réussi.
/// Un tableau non vide → `!empty($result)` = true côté PHP,
/// comme l'objet Rows (toujours truthy) du driver officiel datastax.
fn dml_ack() -> PhpResult<ZBox<ZendHashTable>> {
    let mut ack = ZendHashTable::new();
    ack.push(true)?;
    Ok(ack)
}

/// Récupère `options['arguments']` sous forme de liste positionnelle.
/// PdoCassandra passe toujours un tableau indexé (même pour les marqueurs
/// nommés `:id`, `:md5`, ...), le binding positionnel est donc correct :
/// scylla lie les valeurs dans l'ordre d'apparition des marqueurs.
fn parse_arguments(options: Option<&ZendHashTable>) -> PhpResult<Vec<Option<CqlValue>>> {
    let Some(options) = options else {
        return Ok(Vec::new());
    };
    let Some(args_val) = options.get("arguments") else {
        return Ok(Vec::new());
    };
    let Some(args) = args_val.array() else {
        return Ok(Vec::new());
    };

    let mut values = Vec::with_capacity(args.len());
    for (_, value) in args.iter() {
        values.push(php_scalar_to_cql(value)?);
    }
    Ok(values)
}

// ============================================================================
// Conversions PHP <-> CQL
// ============================================================================

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

fn to_php_exception(message: impl Into<String>) -> PhpException {
    PhpException::default(message.into())
}

// ============================================================================
// Enregistrement du module
// ============================================================================

#[php_module]
pub fn get_module(module: ModuleBuilder) -> ModuleBuilder {
    module
        .class::<Cassandra>()
        .class::<ClusterBuilder>()
        .class::<Cluster>()
        .class::<Session>()
        .class::<PreparedStatement>()
}