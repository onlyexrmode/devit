# SWE-bench (Lite) avec DevIt
Ce dossier fournit tout le nécessaire pour générer un predictions.jsonl et lancer l'évaluation officielle SWE-bench Lite.
⚠️ DevIt génère des diffs depuis le workspace cloné. Pour un démarrage rapide, on évalue d'abord 10 instances de SWE-bench Lite.

## Prérequis
- Docker (pour le harness SWE-bench)
- Python 3.10+
- DevIt compilé et dans le PATH (`cargo build -p devit --release`)
- Un backend LLM local accessible (LM Studio `/v1` ou Ollama `/api/chat` fallback)


## Installation (environnement Python dédié)
python -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt

## Variables d'environnement utiles
# DevIt : approval never + workspace-write (évaluation non interactive)
export DEVIT_TIMEOUT_SECS=600
# Exemple LM Studio :
export DEVIT_BACKEND_URL="http://localhost:1234/v1"
# Exemple Ollama compat (/v1) sinon fallback /api/chat automatique
export DEVIT_BACKEND_URL="http://localhost:11434/v1

## Lancement (smoke 5 instances)
cargo build -p devit --release
export DEVIT_BIN="$PWD/target/release/devit"
export DEVIT_CONFIG="$PWD/bench/devit.bench.toml"
export DEVIT_BACKEND_URL="http://localhost:1234/v1"  # ou 11434/v1 pour Ollama
export DEVIT_TIMEOUT_SECS=120

# générer automatiquement 5 IDs du split test si le fichier n'existe pas
python - <<'PY'
from datasets import load_dataset
ds = load_dataset('princeton-nlp/SWE-bench_Lite', split='test')
with open('bench/instances_auto_5.txt','w') as f:
    for iid in ds.select(range(5))['instance_id']:
        print(iid, file=f)
PY

# génération des prédictions (allow-empty pour smoke)
cd bench
python generate_predictions.py \
  --instances instances_auto_5.txt \
  --output predictions.jsonl \
  --workdir ./workspaces \
  --devit-bin "$DEVIT_BIN" \
  --devit-config "$DEVIT_CONFIG" \
  --dataset princeton-nlp/SWE-bench_Lite \
  --split test \
  --limit 5 \
  --allow-empty

### Évaluation (optionnelle dans le smoke)
make bench-eval

Les résultats (taux de résolution, logs) seront affichés par le harness.

## Notes
Le script clone et checkout chaque repo à base_commit du dataset, puis lance devit suggest --goal ... dans ce workspace.
Tip : commencez par 5–10 instances pour valider le pipeline, puis étendez.
Compat : DevIt est diff-first ; assurez-vous que le backend LLM peut traiter des projets non-Rust (beaucoup d'instances sont Python)

## Conseils pratiques
Backend : pour un run non-interactif, configure devit.toml avec approval = "never" et sandbox = "workspace-write".
Timeouts : ajustez DEVIT_TIMEOUT_SECS selon la taille des tests (ex. 120–900s).
Échelle : commencez par instances_lite_10.txt, puis créez instances_lite_50.txt, etc.
Logs : conservez les sorties predictions.jsonl, logs du harness et JSONL d’audit DevIt (si activé) pour le reporting.

## Relance complète (propre)
# (si besoin) reconstruire le binaire devit
cargo build -p devit --release
export DEVIT_BIN="$PWD/target/release/devit"

# générer 10 IDs valides
python - <<'PY'
from datasets import load_dataset
ds = load_dataset('princeton-nlp/SWE-bench_Lite', split='test')
for iid in ds.select(range(10))['instance_id']:
    print(iid)
PY > bench/instances_lite_10.txt

# (facultatif) backend
export DEVIT_BACKEND_URL="http://localhost:1234/v1"
export DEVIT_TIMEOUT_SECS=600

# génère les prédictions (devit via binaire)
cd bench
python generate_predictions.py \
  --instances instances_lite_10.txt \
  --output predictions.jsonl \
  --workdir ./workspaces \
  --devit-bin "$DEVIT_BIN" \
  --devit-config bench/devit.bench.toml
