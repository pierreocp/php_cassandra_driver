<?php

echo "=== Test appel au vrai cassandra de pharmalia ===\n\n";

try {
    // 1. Créer une connexion avec l'API officielle
    echo "1. Création du cluster builder...\n";
    $cluster = Cassandra::cluster()
        ->withContactPoints('host.docker.internal:9042')
        ->build();
    echo "   ✓ Cluster créé\n\n";

    // 2. Connexion au keyspace system
    echo "2. Connexion au keyspace system...\n";
    $session = $cluster->connect('ocp');
    echo "   ✓ Connecté au keyspace OCP\n\n";

    // 3. Test requête simple
    echo "3. Test requête simple (SELECT cluster_name)...\n";
    $result = $session->query('SELECT cluster_name FROM system.local');
    echo "   ✓ Résultat: " . json_encode($result) . "\n\n";

    // 7. Insérer des données
    // echo "7. Insertion de données...\n";
    // $session->executeSimple("INSERT INTO users (id, name, age) VALUES (1, 'Alice', 30)");
    // $session->executeSimple("INSERT INTO users (id, name, age) VALUES (2, 'Bob', 25)");
    // echo "   ✓ 2 utilisateurs insérés\n\n";

    // 8. Lire les données
    echo "8. Lecture des données...\n";
    $produits = $session->query('SELECT * from produit LIMIT 10');
    echo "   ✓ Résultat: " . json_encode($produits, JSON_PRETTY_PRINT) . "\n\n";

    // 9. Test prepared statement
    echo "9. Test prepared statement...\n";
    $table = 'produit';
    $ids = ['205b86e4-57ec-4a93-85e1-6fe2605396bf', '95af8fe0-cdc7-4a2f-837e-3389be46f50c'];
    $stmt = $this->db->prepare("SELECT * FROM $table WHERE id IN (" . implode(', ', array_map(fn() => '?', $ids)) . ") LIMIT 100");
    $queryOptions['arguments'] = $ids;
    $prepared = $session->prepare('INSERT INTO users (id, name, age) VALUES (?, ?, ?)');
    echo "   ✓ Statement préparé: " . get_class($prepared) . "\n";
    
    // Exécuter avec différents paramètres
    $prepared->execute([10, 'PreparedUser1', 40]);
    $prepared->execute([11, 'PreparedUser2', 45]);
    echo "   ✓ Exécutions multiples avec paramètres différents\n";
    
    // Vérifier
    $verifyStmt = $session->prepare('SELECT * FROM users WHERE id = ?');
    $user10 = $verifyStmt->execute([10]);
    echo "   ✓ Vérification: " . $user10[0]['name'] . " - Age: " . $user10[0]['age'] . "\n\n";

    echo "=== Tous les tests réussis ! ===\n";

} catch (Exception $e) {
    echo "✗ Erreur: " . $e->getMessage() . "\n";
    echo "Stack trace:\n" . $e->getTraceAsString() . "\n";
}
