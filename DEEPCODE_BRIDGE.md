# deepcode bridge boundary

`deep-code-pet` is a desktop shell for deepcode. The deepcode runtime is the
single source of truth for Agent behavior; this repository must not grow a
second Agent implementation.

## Ownership

| Layer | Owns |
| --- | --- |
| deepcode | model selection, authentication, Agent loop, tools, approvals, compaction, retries, timeouts, persona/workspace rules, session persistence, and the runtime event contract |
| bridge in this app | locate the deepcode executable, start `deepcode serve --http`, forward prompts and cancellation, read the SSE stream, and synchronize it to the three webviews |
| pet shell | windows, keyboard and pointer interaction, local pet assets, chat rendering, history presentation, and animation |

The bridge must forward deepcode decisions. It may keep transport state such as
request IDs, revisions, event cursors, and connection recovery, but it must not
infer Agent policy from tool names or session files.

## Runtime transport

For deepcode versions that expose the runtime HTTP server, the bridge starts:

```text
deepcode serve --http --host 127.0.0.1 --port <ephemeral> --auth-token <token>
```

It waits for `GET /health`, then sends `POST /v1/prompt` with a JSON body containing
the prompt. The response is an SSE stream whose `data` values are deepcode runtime
envelopes. The server owns the active session; a selected session is passed with
`--resume <session_id>` when the server starts.

The app keeps the envelope values opaque at the transport boundary. The UI may
project text and lifecycle events into a pose or message, but it must not recreate
tool approval, model routing, session persistence, or retry behavior.

## Migration rule

Any new Agent-facing feature belongs in deepcode first. Add a stable runtime
command or event there, then consume it here. Do not add another parser for
`.deep-code/sessions`, another model override, or another approval policy to this
repository.

The current session index reader and persona bootstrap are compatibility code
retained until deepcode exposes equivalent public desktop operations. The chat
history body already goes through `deepcode session export`. These compatibility
pieces are migration targets, not extension points.
