#!/usr/bin/env bash
set -euo pipefail

# Trigger a real delayed capture via MCP tools/call.
# Defaults:
# - delay: 3000ms (3 seconds)
# - play_sound: true
# - tool: capture_screen
#
# Usage:
#   scripts/trigger-countdown-capture.sh
#   scripts/trigger-countdown-capture.sh 4000
#   ZEUXIS_BIN=target/debug/zeuxis scripts/trigger-countdown-capture.sh 3000

DELAY_MS="${1:-3000}"
BIN="${ZEUXIS_BIN:-zeuxis}"
PROTOCOL_VERSION="2025-06-18"

if ! [[ "$DELAY_MS" =~ ^[0-9]+$ ]]; then
  echo "delay_ms must be a non-negative integer; got: $DELAY_MS" >&2
  exit 1
fi

if [[ "$DELAY_MS" -gt 30000 ]]; then
  echo "delay_ms must be <= 30000; got: $DELAY_MS" >&2
  exit 1
fi

if ! command -v "$BIN" >/dev/null 2>&1; then
  echo "zeuxis binary not found: $BIN" >&2
  echo "Set ZEUXIS_BIN=/absolute/path/to/zeuxis or add zeuxis to PATH." >&2
  exit 1
fi

coproc MCP_SERVER { "$BIN"; }
MCP_READ_FD="${MCP_SERVER[0]}"
MCP_WRITE_FD="${MCP_SERVER[1]}"
MCP_PID="$MCP_SERVER_PID"

cleanup() {
  exec {MCP_WRITE_FD}>&- || true
  exec {MCP_READ_FD}<&- || true
  if kill -0 "$MCP_PID" >/dev/null 2>&1; then
    kill "$MCP_PID" >/dev/null 2>&1 || true
    wait "$MCP_PID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

send() {
  printf '%s\n' "$1" >&"$MCP_WRITE_FD"
}

send "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"${PROTOCOL_VERSION}\",\"capabilities\":{},\"clientInfo\":{\"name\":\"countdown-capture-script\",\"version\":\"1\"}}}"
send "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\",\"params\":{}}"
send "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"capture_screen\",\"arguments\":{\"delay_ms\":${DELAY_MS},\"play_sound\":true}}}"

echo "Waiting for delayed capture response (delay_ms=${DELAY_MS})..."

response_line=""
timeout_secs=$(( (DELAY_MS / 1000) + 30 ))
deadline=$(( SECONDS + timeout_secs ))
while (( SECONDS < deadline )); do
  if IFS= read -r -t 1 line <&"$MCP_READ_FD"; then
    if [[ "$line" == *'"id":2'* ]]; then
      response_line="$line"
      break
    fi
  fi
done

if [[ -z "$response_line" ]]; then
  echo "Timed out waiting for tools/call response." >&2
  exit 1
fi

if command -v jq >/dev/null 2>&1; then
  tool_error="$(printf '%s' "$response_line" | jq -r '.result.isError // false')"
  if [[ "$tool_error" == "true" ]]; then
    error_code="$(printf '%s' "$response_line" | jq -r '.result.structuredContent.error_code // "unknown_error"')"
    message="$(printf '%s' "$response_line" | jq -r '.result.structuredContent.message // "unknown failure"')"
    echo "Capture failed: ${error_code}: ${message}" >&2
    exit 1
  fi

  path="$(printf '%s' "$response_line" | jq -r '.result.structuredContent.path // empty')"
  mode="$(printf '%s' "$response_line" | jq -r '.result.structuredContent.capture_mode // empty')"
  width="$(printf '%s' "$response_line" | jq -r '.result.structuredContent.width // empty')"
  height="$(printf '%s' "$response_line" | jq -r '.result.structuredContent.height // empty')"
  captured_at="$(printf '%s' "$response_line" | jq -r '.result.structuredContent.captured_at_utc // empty')"

  echo "Capture complete."
  echo "mode=${mode} size=${width}x${height} captured_at=${captured_at}"
  echo "path=${path}"
else
  echo "Capture response (install jq for parsed output):"
  echo "$response_line"
fi
