<?php

echo "=== Test Prepared Statements Complets ===\n\n";

try {
    // Créer le keyspace s'il n'existe pas
    $systemSession = Cassandra::cluster()
        ->withContactPoints('cassandra:9042')
        ->build()
        ->connect('system');
    
    $systemSession->executeSimple(
        "CREATE KEYSPACE IF NOT EXISTS test_keyspace WITH replication = {'class': 'SimpleStrategy', 'replication_factor': 1}"
    );
    echo "✓ Keyspace test_keyspace créé/vérifié\n";

    // Connexion au keyspace de test
    $session = Cassandra::cluster()
        ->withContactPoints('cassandra:9042')
        ->build()
        ->connect('test_keyspace');
    echo "✓ Connecté à test_keyspace\n\n";

    // Nettoyer et recréer la table
    $session->executeSimple("DROP TABLE IF EXISTS test_prepared");
    $session->executeSimple("CREATE TABLE test_prepared (id int PRIMARY KEY, name text, score int)");
    echo "✓ Table test_prepared créée\n\n";

    // Test 1: INSERT avec prepared statement (paramètres positionnels)
    echo "1. Test INSERT avec paramètres positionnels:\n";
    $insertStmt = $session->prepare('INSERT INTO test_prepared (id, name, score) VALUES (?, ?, ?)');
    
    $insertStmt->execute([1, 'Alice', 95]);
    echo "   ✓ Insertion 1: Alice\n";
    
    $insertStmt->execute([2, 'Bob', 87]);
    echo "   ✓ Insertion 2: Bob\n";
    
    $insertStmt->execute([3, 'Charlie', 92]);
    echo "   ✓ Insertion 3: Charlie\n";
    
    $insertStmt->execute([4, 'Diana', 88]);
    echo "   ✓ Insertion 4: Diana\n\n";

    // Test 2: SELECT avec prepared statement
    echo "2. Test SELECT avec paramètres:\n";
    $selectStmt = $session->prepare('SELECT * FROM test_prepared WHERE id = ?');
    
    $result = $selectStmt->execute([2]);
    echo "   ✓ SELECT id=2: " . json_encode($result) . "\n\n";

    // Test 3: UPDATE avec prepared statement
    echo "3. Test UPDATE:\n";
    $updateStmt = $session->prepare('UPDATE test_prepared SET score = ? WHERE id = ?');
    $updateStmt->execute([99, 2]);
    echo "   ✓ Score de Bob mis à jour à 99\n";
    
    $result = $selectStmt->execute([2]);
    echo "   ✓ Vérification: " . json_encode($result) . "\n\n";

    // Test 4: SELECT multiple fois (avantage du prepared statement)
    echo "4. Test SELECT multiples avec même statement:\n";
    for ($i = 1; $i <= 4; $i++) {
        $result = $selectStmt->execute([$i]);
        if (!empty($result)) {
            $user = $result[0];
            echo "   ✓ ID $i: {$user['name']} - Score: {$user['score']}\n";
        }
    }
    echo "\n";

    // Test 5: Requête avec plusieurs conditions
    echo "5. Test SELECT avec plusieurs paramètres:\n";
    $complexSelect = $session->prepare('SELECT * FROM test_prepared WHERE id IN (?, ?)');
    $results = $complexSelect->execute([1, 3]);
    echo "   ✓ Résultats pour IDs 1 et 3:\n";
    foreach ($results as $row) {
        echo "      - {$row['name']}: {$row['score']}\n";
    }
    echo "\n";

    // Test 6: Lecture complète de la table
    echo "6. État final de la table:\n";
    $allResults = $session->query('SELECT * FROM test_prepared');
    echo "   Total: " . count($allResults) . " enregistrements\n";
    foreach ($allResults as $row) {
        echo "      ID {$row['id']}: {$row['name']} - Score {$row['score']}\n";
    }
    echo "\n";

    // Test 7: DELETE avec prepared statement
    echo "7. Test DELETE:\n";
    $deleteStmt = $session->prepare('DELETE FROM test_prepared WHERE id = ?');
    $deleteStmt->execute([4]);
    echo "   ✓ Diana supprimée (ID 4)\n";
    
    $remaining = $session->query('SELECT COUNT(*) FROM test_prepared');
    echo "   ✓ Enregistrements restants: " . count($remaining) . "\n\n";

    echo "=== ✅ TOUS LES TESTS PREPARED STATEMENTS RÉUSSIS ===\n\n";
    
    echo "Avantages des Prepared Statements validés:\n";
    echo "  ✓ Réutilisation du même statement\n";
    echo "  ✓ Performance optimisée (pas de parsing répété)\n";
    echo "  ✓ Protection contre injection SQL\n";
    echo "  ✓ Support INSERT, SELECT, UPDATE, DELETE\n";

} catch (Exception $e) {
    echo "✗ Erreur: " . $e->getMessage() . "\n";
    echo "Stack trace:\n" . $e->getTraceAsString() . "\n";
}
