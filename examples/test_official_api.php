<?php

declare(strict_types=1);

// Example using official Cassandra driver API

$hosts = getenv('CASSANDRA_HOSTS') ?: '127.0.0.1';
$keyspace = getenv('CASSANDRA_KEYSPACE') ?: 'php_driver_demo';

// Build cluster and connect (official API style)
$cluster = Cassandra::cluster()
    ->withContactPoints($hosts)
    ->build();

$connection = $cluster->connect($keyspace);

echo "Connected to Cassandra!\n";

// Create keyspace if needed (without keyspace in connect)
$cluster2 = Cassandra::cluster()->withContactPoints($hosts)->build();
$session = $cluster2->connect(null);

$session->execute(
    "CREATE KEYSPACE IF NOT EXISTS {$keyspace} WITH replication = {'class': 'SimpleStrategy', 'replication_factor': 1}"
);

echo "Keyspace created\n";

// Reconnect with keyspace
$connection = $cluster->connect($keyspace);

// Create table
$connection->execute(
    'CREATE TABLE IF NOT EXISTS users (id int PRIMARY KEY, name text, active boolean, score double)'
);

echo "Table created\n";

// Prepare a statement (for better performance)
$stmt = $connection->prepare('INSERT INTO users (id, name, active, score) VALUES (?, ?, ?, ?)');

// Execute with arguments array (like official driver)
$connection->execute($stmt, [
    'arguments' => [1, 'Ada Lovelace', true, 99.5]
]);

$connection->execute($stmt, [
    'arguments' => [2, 'Grace Hopper', false, 85.0]
]);

echo "Data inserted\n";

// Query data
$result = $connection->query('SELECT * FROM users');

echo "Users:\n";
foreach ($result as $row) {
    printf(
        "  - ID: %d, Name: %s, Active: %s, Score: %.1f\n",
        $row['id'],
        $row['name'],
        $row['active'] ? 'yes' : 'no',
        $row['score'] ?? 0.0
    );
}

echo "\nSuccess!\n";
