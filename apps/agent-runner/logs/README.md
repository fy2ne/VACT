# 📊 VACT Telemetry & Execution Logs

This directory contains timestamped session telemetry logs generated automatically by the VACT Autonomous Agent Engine during each execution.

Each mission produces two synchronized log files:
- `mission_YYYY-MM-DD_HH-mm-ss.md`: Human-readable Markdown trace with tables, thoughts, token counts, ms latencies, scene snapshot summaries, and action traces.
- `mission_YYYY-MM-DD_HH-mm-ss.json`: Machine-readable JSON telemetry schema containing all raw model outputs, tokens, and hardware bus traces.
