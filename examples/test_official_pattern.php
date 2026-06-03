<?php

/**
 * Test du pattern officiel demandé :
 * Cassandra::cluster()->withContactPoints('host1,host2')->build()->connect($keyspace)->prepare($sql)->execute($stmt, ['arguments' => [...]])
 */

echo "=== Test Pattern API Officielle ===\n\n";

try {
    // Pattern exact demandé : Cassandra::cluster()->withContactPoints()->build()->connect()
    $session = Cassandra::cluster()
        ->withContactPoints('cassandra:9042')
        ->build()
        ->connect('test_keyspace');
    
    echo "✓ Connexion réussie avec pattern : Cassandra::cluster()->withContactPoints()->build()->connect()\n\n";

    // Préparer un statement
    $prepared = $session->prepare('SELECT * FROM users WHERE id = ?');
    echo "✓ Statement préparé : " . get_class($prepared) . "\n";
    echo "  Note: execute() avec paramètres sera implémenté prochainement\n\n";

    // Pour l'instant, utilisons query() avec valeurs directes
    echo "Test avec query() (sans prepared statement) :\n";
    $result = $session->query("SELECT * FROM users WHERE id = 1");
    echo "  Résultat : " . json_encode($result, JSON_PRETTY_PRINT) . "\n\n";

    // Autre pattern compatible : execute() pour DML
    echo "Test insert avec execute() :\n";
    $session->execute("INSERT INTO users (id, name, age) VALUES (3, 'Charlie', 35)");
    echo "  ✓ Insertion réussie\n\n";

    // Vérification
    $all_users = $session->query("SELECT * FROM users");
    echo "Tous les utilisateurs :\n";
    echo json_encode($all_users, JSON_PRETTY_PRINT) . "\n\n";

    echo "=== Pattern API officielle validé ! ===\n";
    echo "\nAPI implémentée :\n";
    echo "  ✓ Cassandra::cluster()\n";
    echo "  ✓ ->withContactPoints('host1,host2')\n";
    echo "  ✓ ->build()\n";
    echo "  ✓ ->connect(\$keyspace)\n";
    echo "  ✓ ->query(\$sql)\n";
    echo "  ✓ ->execute(\$sql)\n";
    echo "  ✓ ->prepare(\$sql) -> PreparedStatement\n";
    echo "\nÀ implémenter :\n";
    echo "  ⚠️  PreparedStatement->execute(\$params)\n";

} catch (Exception $e) {
    echo "✗ Erreur : " . $e->getMessage() . "\n";
    echo $e->getTraceAsString() . "\n";
}
