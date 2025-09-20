# Approvals hiérarchiques (v0.5)

Lors d’un appel `devit.tool_call`, DevIt vérifie des approvals sur deux clés:

- outer: `devit.tool_call`
- inner: `devit.tool_call:<tool>` (ex. `devit.tool_call:shell_exec`)

Ordre de consommation:

1) inner.once
2) outer.once
3) inner.session
4) outer.session
5) inner.always
6) outer.always

Exemples

- Accorder une exécution unique du shell:

```
devit-mcp --cmd 'devit-mcpd --yes' --call server.approve --json '{"name":"devit.tool_call:shell_exec","scope":"once"}'
```

- Accorder une session pour tout `devit.tool_call`:

```
devit-mcp --cmd 'devit-mcpd --yes' --call server.approve --json '{"name":"devit.tool_call","scope":"session"}'
```

Audit

Chaque consommation d’approval est tracée dans `.devit/journal.jsonl` avec la clé matchée:

```
{ "action":"server.approve.consume", "approval_key":"inner", "name":"devit.tool_call:shell_exec", "hit":"once", ... }
```

Champs notables:

- `approval_key`: `inner` ou `outer` (quelle clé a servi)
- `name`: nom exact de la clé consommée
- `hit`: `once|session|always`

