#!/bin/bash
cd /home/billy/code/workwright
export WW_ROOT=/home/billy/code/itisraining
export WW_PORT=8078
export ANTHROPIC_API_KEY=$(jq -r '.profiles["anthropic:api"].key' /home/billy/.openclaw/agents/main/agent/auth-profiles.json)
setsid ./target/release/ww-server > /tmp/ww-server-itisraining.log 2>&1 &
echo "Workwright started for itisraining.today on port 8078"
