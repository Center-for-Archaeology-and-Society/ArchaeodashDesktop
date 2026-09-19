# OpenCode LiteLLM Authentication Fix

- The OpenCode `asu` provider reads `ASU_AIR_API_KEY` through `"{env:ASU_AIR_API_KEY}"`.
- The Windows user variable had been set to that same placeholder literally, causing LiteLLM to receive `{env:ASU_AIR_API_KEY}` instead of an `sk-...` virtual key.
- The placeholder must be removed or replaced with the real key through a secure user/system environment setting. No key is stored in this repository or Vault.
