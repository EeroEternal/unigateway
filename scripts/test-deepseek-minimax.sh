#!/usr/bin/env bash
# Full test: DeepSeek + MiniMax (non-stream + stream), correct model ids, metrics.
# Usage: start gateway first (e.g. unigateway serve --bind 127.0.0.1:3210), then run this script.

set -e
BIND="${UNIGATEWAY_BIND:-127.0.0.1:3210}"
KEY="${GATEWAY_KEY:-ugk_test_multi}"

echo "=== 1. Health ==="
curl -s "http://${BIND}/health" | jq .
echo ""

echo "=== 2. Request 1 (gateway key -> round-robin; expect DeepSeek, model deepseek-chat) ==="
R1=$(curl -s -X POST "http://${BIND}/v1/chat/completions" \
  -H "Authorization: Bearer ${KEY}" \
  -H "Content-Type: application/json" \
  -d '{"model":"deepseek-chat","messages":[{"role":"user","content":"说一个字：谁"}],"max_tokens":20}')
echo "$R1" | jq -r 'if .choices then .choices[0].message.content // empty else .error.message end'
if echo "$R1" | jq -e '.error' >/dev/null 2>&1; then echo "Response: $R1"; fi
echo ""

echo "=== 3. Request 2 (round-robin -> MiniMax, model m-2.5) ==="
R2=$(curl -s -X POST "http://${BIND}/v1/chat/completions" \
  -H "Authorization: Bearer ${KEY}" \
  -H "Content-Type: application/json" \
  -d '{"model":"m-2.5","messages":[{"role":"user","content":"说一个字：谁"}],"max_tokens":20}')
CONTENT=$(echo "$R2" | jq -r 'if .choices then .choices[0].message.content // empty else .error.message end')
echo "$CONTENT"
if [ -z "$CONTENT" ]; then echo "Raw R2: $R2"; fi
echo ""

echo "=== 3b. Request 3 (round-robin -> Moonshot, model moonshot-v1-8k) ==="
R2b=$(curl -s -X POST "http://${BIND}/v1/chat/completions" \
  -H "Authorization: Bearer ${KEY}" \
  -H "Content-Type: application/json" \
  -d '{"model":"moonshot-v1-8k","messages":[{"role":"user","content":"说一个字"}],"max_tokens":20}')
CONTENT_2b=$(echo "$R2b" | jq -r 'if .choices then .choices[0].message.content // empty else .error.message end')
echo "$CONTENT_2b"
if [ -z "$CONTENT_2b" ]; then echo "Raw R2b: $R2b"; fi
echo ""

echo "=== 4. Request 4 (stream, expect DeepSeek) ==="
R3=$(curl -s -N -X POST "http://${BIND}/v1/chat/completions" \
  -H "Authorization: Bearer ${KEY}" \
  -H "Content-Type: application/json" \
  -d '{"model":"deepseek-chat","messages":[{"role":"user","content":"说一个字"}],"max_tokens":10,"stream":true}')
STREAM_CONTENT=$(echo "$R3" | grep -o '"content":"[^"]*"' | head -5 | sed 's/"content":"//;s/"$//' | tr -d '\n')
echo "Stream content (first pieces): ${STREAM_CONTENT:-<empty or error>}"
if echo "$R3" | head -1 | jq -e '.error' >/dev/null 2>&1; then echo "Stream error: $(echo "$R3" | head -1)"; fi
echo ""

echo "=== 5. Request 5 (stream, expect MiniMax) ==="
R4=$(curl -s -N -X POST "http://${BIND}/v1/chat/completions" \
  -H "Authorization: Bearer ${KEY}" \
  -H "Content-Type: application/json" \
  -d '{"model":"m-2.5","messages":[{"role":"user","content":"说一个字"}],"max_tokens":10,"stream":true}')
STREAM_CONTENT_4=$(echo "$R4" | grep -o '"content":"[^"]*"' | head -5 | sed 's/"content":"//;s/"$//' | tr -d '\n')
echo "Stream content (first pieces): ${STREAM_CONTENT_4:-<empty or error>}"
if [ -z "$STREAM_CONTENT_4" ]; then echo "Raw R4 (first 3 lines): $(echo "$R4" | head -3)"; fi
echo ""

echo "=== 6. Metrics ==="
curl -s "http://${BIND}/metrics" | grep unigateway_requests
echo ""
echo "Done."
echo "  - DeepSeek: model deepseek-chat (non-stream + stream)."
echo "  - MiniMax:  model m-2.5; if you see 'Model Not Exist', try MiniMax-M2.5 or check platform.minimax.io for the current model id."
echo "  - Moonshot: model moonshot-v1-8k (3rd request in round-robin); content may be empty until llm-connector parses Moonshot response."
