#!/usr/bin/env sh
# @aios.schemaVersion 1
# @aios.title Disk Usage Report
# @aios.packageName System Tools
# @aios.description Shows filesystem usage in AIOS DetailView with Copy Output and Re-run actions.
# @aios.mode fullOutput
# @aios.needsConfirmation false
# @aios.icon DF
# @aios.argument1 { "type": "text", "placeholder": "Target path (default: /)", "optional": true }

TARGET="${1:-/}"
echo "Filesystem summary for: $TARGET"
df -h "$TARGET"
