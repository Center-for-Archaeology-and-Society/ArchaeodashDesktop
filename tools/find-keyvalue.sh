#!/usr/bin/env bash
set -uo pipefail
P=$(ls -d ~/.cargo/registry/src/*/parquet-59*/src 2>/dev/null | head -1)
echo "parquet src: $P"
grep -rn "pub struct KeyValue" "$P" 2>/dev/null | head -5
grep -n "set_key_value_metadata" "$P"/file/properties.rs 2>/dev/null | head -5
