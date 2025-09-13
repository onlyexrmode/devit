Checklist à montrer

  - Génération
      - Nombre de lignes: wc -l bench/predictions.jsonl
      - Ex: “5 prédictions générées (patchs vides autorisés pour smoke)”
  - Exécution harness
      - “Evaluating predictions … 100% (1/1)” ou “100% (5/5)”
      - Dossier logs: ls -1 bench/bench_logs et éventuellement tree bench/bench_logs | head -50
  - Résultats rapides
      - Patches appliqués vs échoués:
          - grep -R "Apply patch failed" bench/bench_logs | wc -l
      - Si la version du harness log “Solved”/“success”:
          - grep -R "Solved" bench/bench_logs | wc -l (peut être 0 pour smoke)
      - Garder RUN_ID (log_suffix) pour relancer et comparer.

  Exemple de résumé “rapport”

  - Instances: 5
  - Predictions: 5 lignes
  - Apply patch: 0/5 (patchs vides via --allow-empty)
  - Résolus: 0/5 (attendu pour smoke)
  - Temps total: affiché par le harness
  - Logs: bench/bench_logs/devit-local/…

  Commandes utiles

  - Vérifier la production:
      - wc -l bench/predictions.jsonl
      - head -1 bench/predictions.jsonl | jq .
  - Scanner les logs:
      - grep -R "Apply patch failed" bench/bench_logs | wc -l
      - grep -R "Solved" bench/bench_logs | wc -l || true
      - find bench/bench_logs -name "*.log" -maxdepth 3 | head -5
