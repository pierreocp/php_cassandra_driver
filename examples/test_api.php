<?php

echo "=== Test API Officielle Cassandra Driver ===\n\n";

try {
    // 1. Créer une connexion avec l'API officielle
    echo "1. Création du cluster builder...\n";
    $cluster = Cassandra::cluster()
        ->withContactPoints('cassandra:9042')
        ->build();
    echo "   ✓ Cluster créé\n\n";

    // 2. Connexion au keyspace system
    echo "2. Connexion au keyspace system...\n";
    $session = $cluster->connect('system');
    echo "   ✓ Connecté\n\n";

    // 3. Test requête simple
    echo "3. Test requête simple (SELECT cluster_name)...\n";
    $result = $session->query('SELECT cluster_name FROM system.local');
    echo "   ✓ Résultat: " . json_encode($result) . "\n\n";

    // 4. Créer keyspace de test (avec execute_simple qui ne retourne pas de rows)
    echo "4. Création du keyspace test...\n";
    $session->executeSimple(
        "CREATE KEYSPACE IF NOT EXISTS test_keyspace WITH replication = {'class': 'SimpleStrategy', 'replication_factor': 1}"
    );
    echo "   ✓ Keyspace créé\n\n";

    // 5. Se connecter au keyspace de test
    echo "5. Connexion au keyspace test...\n";
    $testSession = $cluster->connect('test_keyspace');
    echo "   ✓ Connecté\n\n";

    // 6. Créer une table
    echo "6. Création de la table users...\n";
    $testSession->executeSimple(
        "CREATE TABLE IF NOT EXISTS users (id int PRIMARY KEY, name text, age int)"
    );
    echo "   ✓ Table créée\n\n";

    // 7. Insérer des données
    echo "7. Insertion de données...\n";
    $testSession->executeSimple("INSERT INTO users (id, name, age) VALUES (1, 'Alice', 30)");
    $testSession->executeSimple("INSERT INTO users (id, name, age) VALUES (2, 'Bob', 25)");
    echo "   ✓ 2 utilisateurs insérés\n\n";

    // 8. Lire les données
    echo "8. Lecture des données...\n";
    $users = $testSession->query('SELECT * FROM users');
    echo "   ✓ Résultat: " . json_encode($users, JSON_PRETTY_PRINT) . "\n\n";

    // 9. Test prepared statement
    echo "9. Test prepared statement...\n";
    $prepared = $testSession->prepare('INSERT INTO users (id, name, age) VALUES (?, ?, ?)');
    echo "   ✓ Statement préparé: " . get_class($prepared) . "\n";
    
    // Exécuter avec différents paramètres
    $prepared->execute([10, 'PreparedUser1', 40]);
    $prepared->execute([11, 'PreparedUser2', 45]);
    echo "   ✓ Exécutions multiples avec paramètres différents\n";
    
    // Vérifier
    $verifyStmt = $testSession->prepare('SELECT * FROM users WHERE id = ?');
    $user10 = $verifyStmt->execute([10]);
    echo "   ✓ Vérification: " . $user10[0]['name'] . " - Age: " . $user10[0]['age'] . "\n\n";

    echo "=== Tous les tests réussis ! ===\n";

} catch (Exception $e) {
    echo "✗ Erreur: " . $e->getMessage() . "\n";
    echo "Stack trace:\n" . $e->getTraceAsString() . "\n";
}
