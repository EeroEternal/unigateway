#!/usr/bin/env bash
# Moonshot only: cycle round-robin then hit Moonshot with kimi-k2-turbo-preview.
# Usage: start gateway first (e.g. unigateway serve --bind 127.0.0.1:3210), then run this script.

set -e
BIND="${UNIGATEWAY_BIND:-127.0.0.1:3210}"
KEY="${GATEWAY_KEY:-ugk_test_multi}"

echo "=== 1. Health ==="
curl -s "http://${BIND}/health" | jq .
echo ""

echo "=== 2. Cycle RR (2 requests so 3rd will be Moonshot) ==="
curl -s -o /dev/null -X POST "http://${BIND}/v1/chat/completions" \
  -H "Authorization: Bearer ${KEY}" \
  -H "Content-Type: application/json" \
  -d '{"model":"deepseek-chat","messages":[{"role":"user","content":"x"}],"max_tokens":1}'
curl -s -o /dev/null -X POST "http://${BIND}/v1/chat/completions" \
  -H "Authorization: Bearer ${KEY}" \
  -H "Content-Type: application/json" \
  -d '{"model":"m-2.5","messages":[{"role":"user","content":"x"}],"max_tokens":1}'
echo "done."
echo ""

echo "=== 3. Moonshot (moonshot-v1-8k) ==="
R=$(curl -s -X POST "http://${BIND}/v1/chat/completions" \
  -H "Authorization: Bearer ${KEY}" \
  -H "Content-Type: application/json" \
  -d '{"model":"moonshot-v1-8k","messages":[{"role":"user","content":"说一个字"}],"max_tokens":20}')
CONTENT=$(echo "$R" | jq -r 'if .choices then .choices[0].message.content // empty else .error.message end')
echo "$CONTENT"
if [ -z "$CONTENT" ]; then echo "Raw: $R"; fi
echo "Done."
