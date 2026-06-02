<?php

declare(strict_types=1);

$hosts = array_filter(explode(',', getenv('CASSANDRA_HOSTS') ?: '127.0.0.1'));
$port = (int) (getenv('CASSANDRA_PORT') ?: 9042);

$client = new CassandraClient([
    'hosts' => $hosts,
    'port' => $port,
]);

$client->execute("CREATE KEYSPACE IF NOT EXISTS php_driver_demo WITH replication = {'class': 'SimpleStrategy', 'replication_factor': 1}");
$client->execute('CREATE TABLE IF NOT EXISTS php_driver_demo.users (id int PRIMARY KEY, name text, active boolean, score double)');
$client->execute('INSERT INTO php_driver_demo.users (id, name, active, score) VALUES (?, ?, ?, ?)', [1, 'Ada', true, 99.5]);
$client->execute('INSERT INTO php_driver_demo.users (id, name, active, score) VALUES (:id, :name, :active, :score)', [
    'id' => 2,
    'name' => 'Grace',
    'active' => false,
    'score' => null,
]);

$rows = $client->query('SELECT id, name, active, score FROM php_driver_demo.users WHERE id IN (?, ?)', [1, 2]);

var_export($rows);
echo PHP_EOL;
