# AGENTS.md

## Vault Checklist (Required)

For every substantive user interaction in this repository:

1. Start from `Vault/Index.md` and follow only relevant links.
2. Capture a brief interaction summary in the current daily interaction log:
   - File pattern: `Vault/Interaction_Log_YYYY-MM-DD.md`
   - One short bullet per interaction (request + outcome).
3. If the daily interaction log does not exist, create it and link it from `Vault/Interaction_Log.md`.
4. If `Vault/Interaction_Log.md` does not exist, create it and link it from `Vault/Index.md`.
5. For non-trivial changes, create an atomic note and link it from `Vault/Index.md` or an appropriate MOC.
6. Before final response, verify there are no orphan vault notes (every new note linked from an index or MOC).

## Definition

- Substantive interaction: any request that changes code, docs, config, process, or project state.

## GitHub Authentication

- Two fine-grained PATs are in use:
  - **Personal token** (default): stored in `gh` config (`~/.config/gh/hosts.yml`) for `bischrob` repos.
  - **CAS token**: for repos under `Center-for-Archaeology-and-Society`, stored at `~/.secrets/cas_pat` (mode 600, outside any repo).
- When a `gh` command targets a `Center-for-Archaeology-and-Society` repo, run it with `GH_TOKEN=$(cat ~/.secrets/cas_pat) gh ...` so the CAS token takes precedence over the stored personal token.
- Never commit tokens or copy them into the repository, `Vault/`, logs, or command output.

## Security Rule (Vault)

- Never store secrets, credentials, tokens, private keys, PII, or regulated/sensitive data in `Vault/` notes.
