# php_cassandra_driver

Minimal PHP extension written in Rust with [ext-php-rs](https://ext-php.rs/) and backed by the Rust [ScyllaDB driver](https://rust-driver.docs.scylladb.com/). The exposed API is intentionally low level and Cassandra-compatible.

## How to get the `.so`

The extension is a Rust `cdylib`, so the shared object is produced by Cargo.

### Local build

Requirements:

- PHP 8.1+ with development headers (`phpize`/`php-config`)
- Rust stable
- `clang`/`libclang`

```sh
make build
```

The resulting Linux shared object is:

```text
target/release/libphp_cassandra_driver.so
```

You can load it directly for one PHP CLI invocation:

```sh
php -d extension=target/release/libphp_cassandra_driver.so examples/test.php
```

Or copy it into PHP's extension directory:

```sh
make install
```

`make install` copies the file to `$(php-config --extension-dir)/php_cassandra_driver.so`. To enable it permanently, add this line to a PHP ini file loaded by your CLI/FPM installation:

```ini
extension=php_cassandra_driver.so
```

Verify that PHP sees the class:

```sh
php -d extension=target/release/libphp_cassandra_driver.so -r 'var_dump(class_exists("CassandraClient"));'
```

### Docker build only

If you do not want Rust/PHP headers on your host, use Docker BuildKit to export only the extension artifact:

```sh
make docker-build-so
```

That writes:

```text
dist/php_cassandra_driver.so
```

### Docker Compose demo

This starts Cassandra, builds the PHP CLI image with the extension installed, and runs `examples/test.php`:

```sh
make docker-run
```

Cassandra can take a few minutes to become healthy on first startup.

## PHP API

```php
$client = new CassandraClient([
    'hosts' => ['127.0.0.1'], // string or string[]; defaults to 127.0.0.1
    'port' => 9042,           // optional; ignored for hosts that already include :port
    'keyspace' => 'app',      // optional
]);

$rows = $client->query('SELECT id, name FROM users WHERE id = ?', [1]);
$client->execute('INSERT INTO users (id, name) VALUES (:id, :name)', [
    'id' => 1,
    'name' => 'Ada',
]);
```

### Supported values

Bound parameters support simple scalar PHP values only:

- `string` -> CQL `text`
- `int` -> CQL `int` when it fits in 32 bits, otherwise CQL `bigint`
- `float` -> CQL `double`
- `bool` -> CQL `boolean`
- `null` -> CQL `NULL`

`query()` returns rows as PHP associative arrays keyed by Cassandra column name. Common scalar result values are returned as native PHP scalars; other CQL values are stringified for this minimal implementation.

Connection and query failures throw PHP exceptions.

## Configuration

`new CassandraClient(array $config)` accepts:

| Key | Type | Description |
| --- | --- | --- |
| `hosts` | `string|string[]` | Cassandra contact hosts. Defaults to `127.0.0.1`. |
| `port` | `int` | Optional port appended to hosts that do not already include a port. |
| `keyspace` | `string|null` | Optional keyspace selected when the session is opened. |

One Scylla/Cassandra session is created per `CassandraClient` instance. A shared Tokio runtime is used internally by the extension.
