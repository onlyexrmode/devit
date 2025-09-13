Roadmap (4 sprints, 2 semaines)

S1 — Patch-only MVP (CLI + backend + apply)

suggest : génère un diff unifié (déjà dans le squelette).

apply : applique le diff via git apply (dry-run → approval → apply → commit).

Commits conventionnels générés par LLM (feat: …, fix: …).

Config devit.toml lue et validée.
Critères de succès : devit --goal "…" suggest . | devit apply - modifie le repo et commit proprement.

S2 — Approvals + Sandbox

Modes d’approbation : untrusted | on-failure | on-request | never.

Sandbox niveaux : read-only | workspace-write (bwrap/wasmtime intégré derrière un trait ; fallback “no sandbox”).

Journal JSONL des événements (AskApproval, ToolCall, Diff, Error).
Critères : en read-only, apply refuse ; en workspace-write, demande confirmation avec résumé fichiers/commandes.

S3 — Codeexec + Update Plan

codeexec : détection stack (Cargo/npm/CMake) + build/test + timeout + parse JUnit si présent.

update_plan.yaml : étapes, statut, liens commit ; affichage TUI minimal.

Politique on-failure : si test échoue → propose patch correctif.
Critères : devit run --goal "ajoute test" → exécute tests, en cas d’échec propose diff.

S4 — Interop & Qualif

Backends additionnels (LM Studio / llama.cpp server via endpoint OpenAI-like).

MCP client minimal (consommer un outil externe).

Packaging binaire + CI GitHub Actions + README solide + licence Apache-2.0.
Critères : binaire téléchargeable, CI verte, README comparatif, exemple de session reproductible.


Backlog d’issues (prêtes à coder)

CLI: wire “apply”

☐ Lire diff depuis stdin/fichier.

☐ git apply --check (dry-run) → collecter fichiers touchés.

☐ Émettre AskApproval avec résumé (N fichiers, paths).

☐ Si OK: git apply --index, git commit -m "<msg>".

DoD: fonctionne sur repo test, rollback propre si erreur.

Commit message génératif

☐ Prompt LLM court (conventional commits), fallback: feat: <goal>.

DoD: message ≤ 72 chars + body optionnel.

Config & validation

☐ devit.toml → structs + erreurs lisibles, valeurs par défaut.

DoD: démarrage refuse config invalide, affiche aide.

Approval policy (MVP)

☐ untrusted: toujours demander avant write/exec.

☐ on-request: jamais auto (uniquement sur commande explicite).

DoD: tests unitaires sur table de décision.

Sandbox abstraction

☐ Trait Sandbox { dry_run(cmd), exec(cmd), fs_write(...) }.

☐ Impl “noop” + stub bwrap (si présent dans PATH).

DoD: en read-only, fs_write échoue avec message clair.

Audit JSONL

☐ ~/.devit/logs/YYYY-MM-DD.jsonl (Event horodaté).

DoD: chaque apply laisse une trace ToolCall/AskApproval/Result.

Context builder v1

☐ Sélection fichiers pertinents: .rs, Cargo.toml, README, diff git status.

DoD: taille ≤ N Ko, configurable.

Codeexec runner v0

☐ Détecter cargo/npm/cmake.

☐ --dry-run + timeout + capture stdout/stderr.

DoD: devit test exécute la bonne commande selon projet.

TUI minimal (ratatui)

☐ Fenêtre streaming avec sections: Plan | Diff | Logs | Prompts.

DoD: devit run affiche événements en direct.

CI GitHub Actions

☐ Build Linux/macOS, clippy, fmt, tests.

DoD: badge vert + artefacts binaires pour release.

Docs & README

☐ Quickstart 60s, valeurs clés, comparatif succinct.

DoD: un dev peut reproduire la démo offline.

Release v0.1.0 (MVP)

☐ Tag, changelog, binaire, checksum.

DoD: installation mono-binaire documentée.
