#!/usr/bin/env sh
# @aios.schemaVersion 1
# @aios.title System Uptime
# @aios.packageName System Tools
# @aios.description Displays concise host uptime and load average inline in Root Search.
# @aios.mode inline
# @aios.icon UP

if command -v uptime >/dev/null 2>&1; then
    uptime -p 2>/dev/null || uptime
else
    echo "up (uptime unavailable)"
fi
