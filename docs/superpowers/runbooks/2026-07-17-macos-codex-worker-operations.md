# macOS Codex Worker Operations

This runbook operates the Starring Codex worker on the Mac mini. The worker runs
`tools/codex-worker/worker.mjs` as launchd service `local.starring.codex-worker`,
binds only to `127.0.0.1:18181`, and invokes Luna with medium reasoning effort.
The operating limits are two active requests, eight queued requests, and a
55-second request timeout.

The worker is an internal process boundary. Do not publish port `18181`, do not
make it a Cloudflare Tunnel origin, and do not add a public DNS record for it.
An application backend may call it only from the same Mac mini. Public
authentication and product-level admission remain the backend's responsibility.

## Fixed configuration

| Setting | Value |
| --- | --- |
| launchd label | `local.starring.codex-worker` |
| repository | `/Users/jungbogeon/starring` |
| entry point | `tools/codex-worker/worker.mjs` |
| listen address | `127.0.0.1:18181` |
| model | `gpt-5.6-luna` |
| reasoning effort | `medium` |
| active request limit | `2` |
| queue capacity | `8` |
| request timeout | `55000 ms` |
| Keychain service | `com.starring.llm-api-key` |
| Keychain account | `llm-api` |
| launchd template | `ops/macos/local.starring.codex-worker.plist` |
| installed plist | `~/Library/LaunchAgents/local.starring.codex-worker.plist` |
| runtime log | `~/Library/Logs/starring-codex-worker/runtime.log` |
| metrics log | `~/Library/Logs/starring-codex-worker/metrics.jsonl` |

The plist contains identifiers only. The bearer secret and the ChatGPT login
credential stay in Keychain and the Codex login store; neither belongs in the
repository, plist, shell history, logs, or launchd environment.

## Preconditions

Run these checks in a terminal owned by the same logged-in macOS user that will
own the LaunchAgent.

```zsh
cd /Users/jungbogeon/starring
test -x /opt/homebrew/opt/node@24/bin/node
test -x /Applications/ChatGPT.app/Contents/Resources/codex
test -f tools/codex-worker/worker.mjs
/Applications/ChatGPT.app/Contents/Resources/codex login status
security find-generic-password -s com.starring.llm-api-key -a llm-api >/dev/null
plutil -lint ops/macos/local.starring.codex-worker.plist
```

The login status must report an active ChatGPT login. The Keychain command must
exit successfully and must not use `-w` during an existence check. If either
check fails, repair the local login or Keychain item before loading the service.

## Install or update

```zsh
cd /Users/jungbogeon/starring
mkdir -p "$HOME/Library/LaunchAgents" "$HOME/Library/Logs/starring-codex-worker"
install -m 600 ops/macos/local.starring.codex-worker.plist "$HOME/Library/LaunchAgents/local.starring.codex-worker.plist"
plutil -lint "$HOME/Library/LaunchAgents/local.starring.codex-worker.plist"
DOMAIN="gui/$(id -u)"
launchctl bootout "$DOMAIN/local.starring.codex-worker" 2>/dev/null || true
launchctl enable "$DOMAIN/local.starring.codex-worker"
launchctl bootstrap "$DOMAIN" "$HOME/Library/LaunchAgents/local.starring.codex-worker.plist"
launchctl kickstart -k "$DOMAIN/local.starring.codex-worker"
```

`KeepAlive` and `RunAtLoad` keep the worker available after a process exit and
after the user logs in. They do not survive a logged-out user session as a
system daemon would. This Mac mini is configured for operation in its logged-in
user session.

## Smoke test before cutover

Verify launchd state, the loopback-only listener, and health before stopping any
Gemma component.

```zsh
DOMAIN="gui/$(id -u)"
launchctl print "$DOMAIN/local.starring.codex-worker"
lsof -nP -iTCP:18181 -sTCP:LISTEN
API_KEY="$(security find-generic-password -s com.starring.llm-api-key -a llm-api -w)"
curl -fsS http://127.0.0.1:18181/health \
  -H "Authorization: Bearer ${API_KEY}" \
  | jq -e '.status == "ok" and .provider == "codex_chatgpt" and .model == "gpt-5.6-luna" and .reasoning_effort == "medium" and .auth_mode == "chatgpt" and (.active_requests | type) == "number" and (.queued_requests | type) == "number"'
```

The listener output must show `127.0.0.1:18181`. Stop immediately if it shows
`*:18181`, `[::]:18181`, or a non-loopback address.

Exercise one authenticated Luna request without placing the secret in a file or
printing it:

```zsh
curl -fsS http://127.0.0.1:18181/v1/frontier-completions \
  -H 'Content-Type: application/json' \
  -H "Authorization: Bearer ${API_KEY}" \
  --data-binary '{"schema_version":1,"model":"gpt-5.6-luna","reasoning_effort":"medium","messages":[{"role":"user","content":"Return the requested worker smoke status."}],"frontier":{"name":"worker_smoke","description":"Return an object whose status is exactly luna-worker-ok.","parameters":{"type":"object","properties":{"status":{"type":"string","enum":["luna-worker-ok"]}},"required":["status"],"additionalProperties":false}}}' \
  | jq -e '.provider == "codex_chatgpt" and .model == "gpt-5.6-luna" and .reasoning_effort == "medium" and (.tool_call.arguments | fromjson | .status) == "luna-worker-ok"'
unset API_KEY
```

Proceed only if health is successful, the authenticated request succeeds, the
response names the requested model, and the runtime log contains no credential
or repeated restart loop.

## Cut over from the local Gemma stack

The existing public raw-LLM path is
`local.cloudflared.starring` to `local.llm-api` on port `18080`, then
`local.ollama.server` on port `11434`. The new Codex worker is not a public
replacement for that route. Any backend moving to Luna must first be configured
to call `127.0.0.1:18181` locally and pass its smoke test.

Stopping `local.llm-api` aborts active and queued requests. Before the change,
inspect its health output and wait for `active_requests` and `queued_requests` to
reach zero when those fields are present.

```zsh
curl -fsS http://127.0.0.1:18080/health | jq
```

After the new worker smoke test succeeds and the old gateway is drained, stop
the old stack in this order:

```zsh
DOMAIN="gui/$(id -u)"
launchctl disable "$DOMAIN/local.llm-api"
launchctl bootout "$DOMAIN/local.llm-api"
ollama stop gemma4:12b-mlx
launchctl disable "$DOMAIN/local.ollama.server"
launchctl bootout "$DOMAIN/local.ollama.server"
```

Pairing `disable` with `bootout` is intentional. `bootout` alone would allow a
`RunAtLoad` LaunchAgent to return at the next login.

The Cloudflare service may be stopped only after confirming that its remotely
managed tunnel has no ingress other than the retired raw LLM endpoint. If it is
raw-LLM-only, stop it last:

```zsh
DOMAIN="gui/$(id -u)"
launchctl disable "$DOMAIN/local.cloudflared.starring"
launchctl bootout "$DOMAIN/local.cloudflared.starring"
```

If the tunnel also serves another hostname or service, leave the LaunchAgent
running and remove or replace only the raw LLM ingress through the Cloudflare
configuration. Never change that ingress to `127.0.0.1:18181`.

## Post-cutover verification

```zsh
DOMAIN="gui/$(id -u)"
launchctl print "$DOMAIN/local.starring.codex-worker"
launchctl print-disabled "$DOMAIN"
lsof -nP -iTCP:18181 -sTCP:LISTEN
lsof -nP -iTCP:18080 -sTCP:LISTEN
lsof -nP -iTCP:11434 -sTCP:LISTEN
ollama ps
API_KEY="$(security find-generic-password -s com.starring.llm-api-key -a llm-api -w)"
curl -fsS http://127.0.0.1:18181/health -H "Authorization: Bearer ${API_KEY}" | jq
unset API_KEY
```

The worker must be healthy and bound only to loopback. Ports `18080` and `11434`
must have no listeners, `ollama ps` must list no loaded model, and the disabled
map must contain the retired services. If Cloudflare was raw-LLM-only, its label
must also be disabled and absent from the running launchd domain.

## Rollback

Rollback restores the previous local gateway without downloading or deleting a
model. Stop the Codex worker first, then restore Ollama, the local gateway, and
the tunnel in dependency order.

```zsh
DOMAIN="gui/$(id -u)"
launchctl disable "$DOMAIN/local.starring.codex-worker"
launchctl bootout "$DOMAIN/local.starring.codex-worker"
launchctl enable "$DOMAIN/local.ollama.server"
launchctl bootstrap "$DOMAIN" "$HOME/Library/LaunchAgents/local.ollama.server.plist"
launchctl kickstart -k "$DOMAIN/local.ollama.server"
until curl -fsS http://127.0.0.1:11434/api/version >/dev/null; do sleep 1; done
launchctl enable "$DOMAIN/local.llm-api"
launchctl bootstrap "$DOMAIN" "$HOME/Library/LaunchAgents/local.llm-api.plist"
launchctl kickstart -k "$DOMAIN/local.llm-api"
until curl -fsS http://127.0.0.1:18080/health >/dev/null; do sleep 1; done
```

If the Cloudflare service was disabled during cutover, restore it only after the
local gateway is healthy:

```zsh
DOMAIN="gui/$(id -u)"
launchctl enable "$DOMAIN/local.cloudflared.starring"
launchctl bootstrap "$DOMAIN" "$HOME/Library/LaunchAgents/local.cloudflared.starring.plist"
launchctl kickstart -k "$DOMAIN/local.cloudflared.starring"
```

The first Gemma request may need up to 120 seconds while the model is loaded.
Keep the installed model files for this rollback path. Do not run
`ollama rm gemma4:12b-mlx`, delete Ollama blobs, or remove the existing legacy
LaunchAgent plists during the Luna trial.

## Routine operations

```zsh
DOMAIN="gui/$(id -u)"
launchctl kickstart -k "$DOMAIN/local.starring.codex-worker"
tail -n 200 "$HOME/Library/Logs/starring-codex-worker/runtime.log"
launchctl print "$DOMAIN/local.starring.codex-worker"
```

Repeated launchd exits require inspecting the runtime log and `last exit code`
before restarting again. A queue-full response is admission control, not a
reason to increase concurrency without a measured load test. A timeout must
terminate its Codex child process; verify that no orphaned Codex process remains
before retrying.
