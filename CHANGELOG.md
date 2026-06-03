# Changelog

## [Unreleased] - 2026-06-02

### ✅ Added - Official Cassandra Driver API

Implemented official Cassandra PHP driver compatible API:

#### New Classes
- **Cassandra** - Static entry point
  - `cluster()` : Returns ClusterBuilder instance

- **CassandraClusterBuilder** - Fluent builder pattern
  - `withContactPoints(string $hosts)` : Set contact points (comma-separated)
  - `withPort(int $port)` : Set default port
  - `build()` : Create and return Cluster instance

- **CassandraCluster** - Cluster representation
  - `connect(string $keyspace)` : Connect to keyspace and return Session

- **CassandraSession** - Active database session
  - `query(string $cql)` : Execute SELECT query, returns array of rows
  - `execute(string $cql)` : Execute DML/DDL statement
  - `executeSimple(string $cql)` : Execute without parsing results
  - `prepare(string $cql)` : Prepare statement, returns PreparedStatement

- **CassandraPreparedStatement** - Prepared statement
  - `execute(array $params)` : Execute prepared statement with positional parameters
  - Supports INSERT, SELECT, UPDATE, DELETE operations
  - Optimized for multiple executions with different parameters

#### Usage Pattern
```php
$session = Cassandra::cluster()
    ->withContactPoints('host1:9042,host2:9042')
    ->build()
    ->connect('my_keyspace');

$users = $session->query('SELECT * FROM users');
$session->execute("INSERT INTO users (id, name) VALUES (1, 'Alice')");
$prepared = $session->prepare('INSERT INTO users VALUES (?, ?)');
```

### 🔧 Fixed

- **DDL/DML handling**: `executeSimple()` now returns empty array for non-SELECT statements instead of throwing error
- **PreparedStatement import**: Fixed import path to `scylla::statement::prepared::PreparedStatement`
- **ArrayKey matching**: Removed non-existent `ZendString` variant, added catch-all pattern
- **Rust version**: Updated Dockerfile to force latest stable Rust (1.96.0) for edition2024 support

### ✅ Implemented

- **PreparedStatement->execute($params)** : Fully functional with positional parameters
- All CRUD operations supported (INSERT, SELECT, UPDATE, DELETE)
- Statement reuse for performance optimization
- Protection against SQL injection

### 🐛 Known Issues

- Named parameters in prepared statements not yet supported (positional only)
- Complex Cassandra types (UUID, Timestamp, Collections) not yet supported
- No connection pooling
- No pagination support for large result sets

### 🧪 Tests

Added comprehensive test files:
- `examples/test_api.php` - Full API workflow test
- `examples/test_official_pattern.php` - Official driver pattern validation
- `examples/test_prepared_statements.php` - Complete prepared statements test suite
  - INSERT, SELECT, UPDATE, DELETE operations
  - Statement reuse validation
  - Multiple parameter bindings

### 📦 Dependencies

- ext-php-rs: 0.15.14
- scylla: 1.6.0
- tokio: 1.52.3
- once_cell: 1.21.4

### 🏗️ Build

- Docker multi-stage build with Rust 1.96.0
- ScyllaDB for testing (compatible with Cassandra)
- PHP 8.3 CLI runtime

### 📝 Documentation

- Updated README.md with official API documentation
- Added examples for new API pattern
- Preserved backward compatibility with CassandraClient

## [Previous] - Legacy API

### CassandraClient (Original Implementation)

Simple connection-oriented API:
```php
$client = new CassandraClient(['hosts' => ['127.0.0.1'], 'keyspace' => 'app']);
$rows = $client->query('SELECT * FROM users');
```

Still available and functional alongside the new official API.
