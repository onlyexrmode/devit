#!/usr/bin/env python3
import argparse
import json
import os
import shutil
import subprocess
from pathlib import Path

from datasets import load_dataset
from git import Repo
from tqdm import tqdm

# Champs typiques utilisés dans SWE-bench Lite
# - instance_id : identifiant unique
# - repo : nom github "owner/repo"
# - base_commit : commit SHA sur lequel appliquer le patch
# - problem_statement : description de l'issue / tâche
# Certains jeux ont aussi "title" et/ou "hints" ; on les concatène si présents.


def run(cmd, cwd=None, check=True, env=None):
    proc = subprocess.run(cmd, cwd=cwd, text=True, capture_output=True, env=env)
    if check and proc.returncode != 0:
        raise RuntimeError(f"Command failed: {' '.join(cmd)}\nSTDOUT:\n{proc.stdout}\nSTDERR:\n{proc.stderr}")
    return proc


def ensure_workspace(repo_slug: str, commit_sha: str, workdir: Path) -> Path:
    """Clone (si besoin) et checkout le commit de base pour l'instance."""
    target = workdir / repo_slug.replace('/', '__') / commit_sha
    code_dir = target / 'code'
    if code_dir.exists():
        return code_dir

    target.mkdir(parents=True, exist_ok=True)
    url = f"https://github.com/{repo_slug}.git"
    repo_path = target / 'repo'
    Repo.clone_from(url, repo_path)
    repo = Repo(repo_path)
    repo.git.checkout(commit_sha)

    # Copie de travail (évite de salir le repo cloné si l'agent écrit)
    shutil.copytree(repo_path, code_dir)
    return code_dir


def build_goal(record: dict) -> str:
    parts = []
    if 'title' in record and record['title']:
        parts.append(str(record['title']))
    if 'problem_statement' in record and record['problem_statement']:
        parts.append(str(record['problem_statement']))
    # SWE-bench Lite → le champ s'appelle "hints_text"
    if 'hints_text' in record and record['hints_text']:
        parts.append(str(record['hints_text']))
    goal = '\n\n'.join(parts).strip()
    if not goal:
        goal = f"Resolve issue {record.get('instance_id','')}"
    return goal


def resolve_devit_cmd(explicit_bin: str | None = None):
    """
    Résout la commande DevIt à exécuter.
    Ordre de priorité :
      1) --devit-bin (chemin vers binaire)
      2) $DEVIT_BIN (chemin vers binaire)
      3) $DEVIT_REPO (utilise `cargo run -p devit --`)
      4) 'devit' dans le PATH
    """
    if explicit_bin and os.path.isfile(explicit_bin) and os.access(explicit_bin, os.X_OK):
        return [explicit_bin], None
    env_bin = os.environ.get("DEVIT_BIN")
    if env_bin and os.path.isfile(env_bin) and os.access(env_bin, os.X_OK):
        return [env_bin], None
    devit_repo = os.environ.get("DEVIT_REPO")
    if devit_repo and os.path.isdir(devit_repo):
        return (["cargo", "run", "-p", "devit", "--"], devit_repo)
    # fallback PATH
    if shutil.which("devit"):
        return ["devit"], None
    raise FileNotFoundError(
        "DevIt introuvable. Fournis --devit-bin=/path/to/devit "
        "ou exporte DEVIT_BIN, ou DEVIT_REPO pour utiliser `cargo run`."
    )

def devit_suggest(goal: str, cwd: Path, devit_bin: str | None = None, devit_config: str | None = None) -> str:
    """Appelle DevIt et retourne le diff unifié (stdout)."""
    env = os.environ.copy()
    # Si fourni, passer un chemin de config unique
    if devit_config:
        env["DEVIT_CONFIG"] = devit_config
    cmd, repo_root = resolve_devit_cmd(devit_bin)
    full_cmd = cmd + ["suggest", "--goal", goal, str(cwd)]
    if repo_root:
        # si on passe par cargo run, on lance depuis la racine du repo DevIt
        proc = run(full_cmd, cwd=repo_root, check=False, env=env)
    else:
        proc = run(full_cmd, cwd=cwd, check=False, env=env)
    return proc.stdout.strip()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--instances', required=False, help='Fichier texte avec une instance_id par ligne (facultatif si --limit est fourni)')
    ap.add_argument('--output', default='predictions.jsonl')
    ap.add_argument('--workdir', default='./workspaces')
    ap.add_argument('--dataset', default='princeton-nlp/SWE-bench_Lite')
    ap.add_argument('--split', default='test', help='Split du dataset (ex: test, validation)')
    ap.add_argument('--limit', type=int, default=0, help='Échantillonner N instances du split (si --instances non fourni)')
    ap.add_argument('--allow-empty', action='store_true', help='Écrire une entrée JSONL même si le diff est vide (smoke)')
    ap.add_argument('--devit-bin', default=None, help='Chemin explicite vers le binaire devit (sinon DEVIT_BIN/DEVIT_REPO/PATH)')
    ap.add_argument('--devit-config', default=os.environ.get("DEVIT_CONFIG"), help='Chemin vers devit.toml à utiliser pour tous les runs')
    args = ap.parse_args()

    workdir = Path(args.workdir).resolve()
    workdir.mkdir(parents=True, exist_ok=True)

    # Charger la liste d'instances si fournie, sinon auto-générer via --limit
    target_ids: list[str] = []
    if args.instances:
        with open(args.instances) as f:
            target_ids = [ln.strip() for ln in f if ln.strip() and not ln.startswith('#')]
        print(f"Loaded {len(target_ids)} instance ids from {args.instances}")
    # Charger le dataset
    ds = load_dataset(args.dataset, split=args.split)
    if not target_ids:
        n = args.limit if args.limit and args.limit > 0 else 1
        auto_file = Path(f"instances_auto_{n}.txt")
        ids = ds.select(range(min(n, len(ds))))['instance_id']
        with auto_file.open('w') as f:
            for iid in ids:
                f.write(iid + "\n")
        target_ids = list(ids)
        print(f"Auto-selected {len(target_ids)} ids to {auto_file}")
    # indexer par instance_id
    index = {rec['instance_id']: rec for rec in ds}

    out_path = Path(args.output)
    if out_path.exists():
        out_path.unlink()

    with out_path.open('w') as fw:
        for iid in tqdm(target_ids, desc='instances'):
            rec = index.get(iid)
            if rec is None:
                print(f"[WARN] instance not found in dataset: {iid}")
                continue
            repo_slug = rec['repo']
            base_commit = rec['base_commit']
            goal = build_goal(rec)

            # Préparer workspace
            code_dir = ensure_workspace(repo_slug, base_commit, workdir)

            # Générer le diff via DevIt (patch-only)
            diff = devit_suggest(goal, code_dir, devit_bin=args.devit_bin, devit_config=args.devit_config)
            if not diff:
                print(f"[WARN] empty diff for {iid}")
                if not args.allow_empty:
                    continue
                else:
                    diff = ""

            # Ligne JSONL selon le format SWE-bench
            line = {
                "instance_id": iid,
                "model_name_or_path": "devit-local",
                "model_patch": diff,
            }
            fw.write(json.dumps(line) + '\n')

    print(f"✔ Wrote {out_path}")

if __name__ == '__main__':
    main()
