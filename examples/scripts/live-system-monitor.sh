#!/usr/bin/env sh
# @aios.schemaVersion 1
# @aios.title Live System Monitor
# @aios.packageName System Tools
# @aios.description Live snapshot of CPU load, memory usage, and top processes auto-refreshing every 2s.
# @aios.mode fullOutput
# @aios.refreshTime 2s
# @aios.needsConfirmation false
# @aios.icon utilities-system-monitor
# @aios.argument1 { "type": "text", "placeholder": "Sort by cpu or mem (default: cpu)", "optional": true }

SORT_KEY="${1:-cpu}"
case "$SORT_KEY" in
    mem|MEM|memory|rss)
        PS_SORT="-%mem"
        SORT_LABEL="memory"
        ;;
    *)
        PS_SORT="-%cpu"
        SORT_LABEL="CPU"
        ;;
esac

echo "Updated: $(date '+%Y-%m-%d %H:%M:%S')  |  Sort: $SORT_LABEL"
if [ -r /proc/loadavg ]; then
    read -r L1 L5 L15 PROCS _ < /proc/loadavg
    echo "Load Avg: $L1 (1m)  $L5 (5m)  $L15 (15m)  |  Tasks: $PROCS"
fi

echo ""
echo "[Memory]"
if command -v free >/dev/null 2>&1; then
    free -h
else
    grep -E '^(MemTotal|MemAvailable|SwapTotal|SwapFree):' /proc/meminfo 2>/dev/null || echo "memory info unavailable"
fi

echo ""
echo "[Top Processes by $SORT_LABEL]"
if command -v ps >/dev/null 2>&1; then
    ps -eo pid,comm:20,%cpu,%mem,rss --sort="$PS_SORT" | head -n 9
else
    echo "ps unavailable"
fi
