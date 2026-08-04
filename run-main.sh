#!/bin/bash
cd /home/billy/code/workwright
export WW_ROOT=/home/billy/code/workwright
export WW_PORT=8077
export WW_DEPLOY_HOST="billy@157.245.142.142"
# Anthropic key lives in the OpenClaw agent sqlite store (auth migrated from JSON in 2026-06)
export ANTHROPIC_API_KEY=$(sqlite3 /home/billy/.openclaw/agents/main/agent/openclaw-agent.sqlite \
  "SELECT store_json FROM auth_profile_store WHERE store_key='primary';" \
  | python3 -c "import json,sys; print(json.load(sys.stdin)['profiles']['anthropic:api']['key'])")
setsid ./target/release/ww-server > /tmp/ww-server-rust.log 2>&1 &
echo "Main Workwright started on port 8077"
