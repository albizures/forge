# Provider retry semantics in pi-ai and pi-durable 1.5.2

Research for [#33](https://github.com/albizures/forge/issues/33). Facts only; no policy is
recommended here (that is for [#34](https://github.com/albizures/forge/issues/34)).

Sources are the installed packages at `main` `c00b076`:
`runner/node_modules/@earendil-works/{pi-ai,pi-durable,chord}` (all 1.5.2),
`runner/node_modules/openai` (7.19.0, the SDK pi-ai uses), Forge's runner and crates, and the
Node.js runtime pinned by `runner/package.json` (`engines.node` 24.18.0). Node internals were
read from the running binary via `process.binding('natives')`; their line numbers refer to that
embedded source. Paths below are relative to `runner/node_modules/` unless they start with
`runner/`, `crates/` or `docs/`.

## Summary

- There are **three retry layers**, none of which sends an idempotency key:
  1. OpenAI SDK built-in retries (default 2). pi-ai always sets them to **0**.
  2. pi-ai's in-request retry `retryProviderRequest` (default 0 retries). It wraps only the
     request up to response headers. It is invisible to the durable log.
  3. pi-durable's generation-attempt retry `settings.retry` (default **enabled**, 3 retries). It
     commits the failed assistant message to the log, sleeps durably, and sends a **new**
     request.
- Forge turns off layers 2 and 3 (`maxRetries: 0` forced at provider dispatch;
  `settings.retry.enabled: false`).
- Retryable classes differ by layer. Layers 1 and 2 use HTTP status: no status (a connection
  error), 408, 409, 429, 5xx, or `x-should-retry`. Layer 3 matches a regex against the error
  **string**.
- `Connection error.: AggregateError (ETIMEDOUT)` (release verification run 2) is retryable in
  every layer. In Node 24.18.0 an `AggregateError` with `ETIMEDOUT` comes only from
  `net.connect` failing **every** TCP connection attempt while the socket is still connecting.
  At that point no TLS session exists and no HTTP request bytes have been written. No layer
  records that distinction, though. pi-ai reduces the error object to a string before pi-durable
  or Forge sees it. Other `Connection error.: …` causes (for example `other side closed`) can
  occur after the request was written.
- On a durable retry the failed attempt becomes an assistant entry with `stopReason: "error"` in
  the log, and pi-ai omits it from the next request's context. With retry disabled, the run ends
  `model_error`, the input settles `unanswered`, and Forge records Missing Outcome.

## 1. Where Forge disables retries

`runner/src/profiles.ts`:

- L101–102 (comment): "Only the selected model and immediate chat APIs are allowed; retries
  and ambient API env are off."
- L110–131 `immediateOpenAI` proxies the OpenAI provider. For `stream`/`streamSimple`, L120
  calls the real method with `{ ...options, maxRetries: 0, env: {} }`. The override comes last,
  so it beats anything pi-durable passes from `settings.stream.maxRetries`.
- L121–129: events are re-pushed after `redact()` (L103–108 replaces the API key in strings).
  If iterating the source stream **throws**, Forge substitutes a synthetic error message with
  `errorMessage: 'durable-model-request-failed'` (L126). pi-ai normally reports failures as an
  `error` event rather than throwing (§2.3), so in that normal path pi-ai's own `errorMessage`
  passes through unchanged except for redaction.
- L168: Harness settings `retry: { enabled: false }` (and compaction disabled).
- L147: the provider is wrapped as `guardProvider(immediateOpenAI(openaiProvider(), …), authorization)`.
  `runner/src/authorization.ts` L185–199: `guardProvider` calls `authorization.admit()` and
  combines the lease's `AbortSignal` **once per `stream`/`streamSimple` call**.

Docs already state the consequence: `docs/release-verification/6a2196a.md` L77–81 and L111–113,
and `docs/durable-execution.md` L510–512 ("Model retries stay disabled, so an in-flight model
request may settle unanswered").

## 2. pi-ai 1.5.2

### 2.1 Which API path Forge uses

`@earendil-works/pi-ai/dist/providers/openai.js` L27: the `openai` provider uses
`openAIResponsesApi()`, the OpenAI Responses API. The request path is
`@earendil-works/pi-ai/dist/api/openai-responses.js` `stream` (L108–194).

### 2.2 Layer 1: OpenAI SDK retries are always off

`openai-responses.js` L145–149 builds `requestOptions` with `maxRetries: 0` unconditionally. The
comment in `@earendil-works/pi-ai/dist/utils/provider-retry.js` L68–75 explains why: the SDK's
backoff timers ignore the request `AbortSignal`. pi-ai therefore calls the SDK with
`maxRetries: 0` and does its own retrying.

For reference, the SDK's own policy (`openai/client.js`, 7.19.0):

- Default `maxRetries` is 2 (L242).
- A connection-level failure, meaning `fetch` rejects before a response, is retried when
  retries remain and the body is not streaming (L784–805). That covers any `fetch` error, not
  only timeouts. When retries run out it throws `APIConnectionTimeoutError` if
  `/timed? ?out/i` matches the error or its cause (L793–794, L822–836). Otherwise it throws
  `APIConnectionError` (L840–843), whose default message is `"Connection error."`
  (`openai/core/error.js` L78–86; `status` and `headers` are `undefined`).
- An HTTP response is retried per `shouldRetry` (L1026–1046): `x-should-retry: true|false`
  wins. Otherwise it retries on 408, 409, 429 and ≥ 500.
- **Idempotency:** the SDK can send an idempotency header (L1195–1200, key
  `stainless-node-retry-<uuid>` from L425–427), but only when the client's `idempotencyHeader`
  property is set. It is declared optional (`openai/client.d.ts` L200) and nothing in the
  installed SDK assigns it, so the OpenAI client sends no idempotency key.

### 2.3 Layer 2: `retryProviderRequest` (in-request, invisible to the log)

`@earendil-works/pi-ai/dist/utils/provider-retry.js`:

- L76–96 `retryProviderRequest(request, { maxRetries, maxRetryDelayMs, signal, noRetryStatuses })`.
  The default is **0 retries** (L77). Each retry is a fresh SDK request (L81 comment), and the
  backoff sleep can be aborted (L51–67, L93).
- L2–7 `isProviderError`: an `Error` with `status` and `headers` fields (the OpenAI
  `APIError` family, including `APIConnectionError`).
- L9–21 `isRetryableProviderError` ("Mirrors the pinned OpenAI/Anthropic SDK retry policy"):
  `x-should-retry` wins. Otherwise it retries on `status === undefined` (every connection-level
  error), 408, 409, 429 and ≥ 500.
- L29–45 delay: `retry-after-ms`, then `retry-after`, then exponential `min(0.5·2^i, 8)` s
  with up to 25% jitter. A server-requested delay above `maxRetryDelayMs` (default 60 s, L1,
  L22–28) throws immediately.

Scope in `openai-responses.js` L150–154: it wraps only
`client.responses.create(params, requestOptions).withResponse()`, which resolves once response
**headers** arrive. Body streaming (`processResponsesStream`, L157–162) is outside the wrapper,
so a failure after headers, mid-stream, is never retried by this layer. `maxRetries` and
`maxRetryDelayMs` come from the caller's stream options (L151–152;
`@earendil-works/pi-ai/dist/api/simple-options.js` L52–53 forwards them for `streamSimple`).
Option docs: `@earendil-works/pi-ai/dist/types.d.ts` L89–105.

Retries at this layer happen inside one `stream` call. pi-durable cannot see them, and neither
can Forge's `guardProvider`: `authorization.admit()` is not rechecked, though the lease signal
can abort the backoff.

Error to message: on any failure, `openai-responses.js` L175–190 sets `stopReason` to `"error"`
(or `"aborted"` if the signal fired). It pushes an `error` event whose `errorMessage` is a
**string** from `formatProviderError(normalizeProviderError(error), …)`. The original error
object, with its cause chain, is not passed on. `@earendil-works/pi-ai/dist/utils/error-body.js`:

- L17–32: with no status and no body, the message is `withCauseDetail(error)`.
- L41–58 `withCauseDetail`: follows `cause` up to 5 levels, takes the **deepest** cause's
  `message || name`, and appends its `code`. For the observed error this gives
  `Connection error.` + `: ` + `AggregateError` (empty message, so the name is used) +
  ` (ETIMEDOUT)`.

### 2.4 Retry classifier used by pi-durable (string-based)

`@earendil-works/pi-ai/dist/utils/retry.js`:

- L208–215 `isRetryableAssistantError(message)`: `true` only if `stopReason === "error"`, the
  `errorMessage` does not match the non-retryable list, and it does match the retryable list.
  The L199–207 doc comment says it "does not implement retry policy".
- L4–28 non-retryable list: quota, billing and subscription limits (`insufficient_quota`,
  `quota exceeded`, `billing`, `Monthly usage limit reached`, …).
- L29–112 retryable list (case-insensitive), in these groups:
  - overload and rate limiting (`overloaded`, `rate.?limit`, `too many requests`, `429`)
  - 5xx codes and status text (`500|502|503|504|520|524`, `service … unavailable`,
    `server.?error`, `internal.?error`)
  - gateway and upstream text
  - network and transport (`network.?error`, `connection.?error` L61, `connection.?refused`,
    `other side closed` L64, `fetch failed`, `ENOTFOUND`, `EAI_AGAIN`, `reset before headers`,
    `socket hang up`, `timed? out`, `timeout` L77, `terminated` L78)
  - premature stream ends (`ended without`, `stream ended before …`,
    `stream disconnected before completion`)
  - explicit retry guidance in the text ("you can retry your request", …)
- `Connection error.: AggregateError (ETIMEDOUT)` matches `connection.?error`, so it is
  retryable. Forge's synthetic `durable-model-request-failed` matches no pattern, so it is not
  retryable.
- L114–118 `retryDelayMs(policy, attempt) = min(baseDelayMs·2^(attempt−1), maxAgentDelayMs ?? 60 000)`.
- L155–198 `retryAssistantCall` is a separate retry loop. pi-durable does not use it; pi-durable
  imports only `isRetryableAssistantError` and `retryDelayMs`
  (`@earendil-works/pi-durable/dist/harness/generation.js` L3).

## 3. pi-durable 1.5.2: Layer 3, generation-attempt retry

### 3.1 Settings

`@earendil-works/pi-durable/dist/harness/agent.js` L4–9 `DEFAULT_RETRY_POLICY = { enabled: true, maxRetries: 3, baseDelayMs: 2000, maxAgentDelayMs: 60000 }`,
merged with the host's `settings.retry` at L47. Types are in
`@earendil-works/pi-durable/dist/harness/types.d.ts`:

- L510–515 `ConversationRetryPolicy`: "Durable generation attempt retries".
- L500–502 `settings.stream.maxRetries`: "Provider/SDK retries inside one request attempt",
  which is layer 2.

### 3.2 Where the retry sits relative to the durable log

`GenerationTask` (`@earendil-works/pi-durable/dist/harness/generation.js` L24–) runs in phases:

1. `prepare` (L39–119) commits system entries and the checkpoint
   `{ phase: "request", attempt, … }` (L110–118). This happens **before** any provider call.
2. `request` (L121–160):
   - It first checks `task.restarts`. If the host stopped while this request was in flight, the
     request is "a failed attempt, not a resend" (L125–127 → `interrupted`).
   - Next it converts any leftover partial into an aborted entry and records `live.generation`
     (L131–136).
   - Then it streams via `runtime.models.streamSimple` (L146–157; `streamResponse` L506–567).
     Partial output is committed at most every `progress.partialIntervalMs`.
3. `classify` (L569–655) handles the terminal message:
   - A context overflow goes to compaction (L606–623) and is never retried (L626).
   - The retry condition (L625–631) is `stopReason === "error" && !overflow &&
     isRetryableAssistantError(message) && policy.enabled && attempt <= policy.maxRetries`.
   - One commit (L633–654) **appends the error message as an assistant entry**
     (`appendAssistant`, L637).
   - On retry, the commit sets `live.generation.retry = { at, error }` and sleeps durably with
     checkpoint `{ phase: "retry", attempt, until }` (L638–647).
   - Otherwise the run ends with `reason: "model_error"` (L648–653).
4. `retry` (L161–184) runs after the backoff. It commits `{ phase: "prepare", attempt: attempt + 1 }`.
   The next attempt rebuilds the request from the log. A deferred poll instead polls the same
   handle again.

The next request does not resend the failed attempt's output.
`@earendil-works/pi-ai/dist/api/transform-messages.js` L176–180 skips assistant messages with
`stopReason` `"error"` or `"aborted"` when building provider context.

Host death mid-request (`interrupted`, `generation.js` L341–388; README L216; CHANGELOG
L139, L203) is charged as a failed attempt under the same `settings.retry`:

- If a retry is left (`policy.enabled && attempt <= maxRetries`, L349), the task sleeps and then
  re-prepares.
- Otherwise the run ends with `reason: "interrupted"`, and its inputs settle `unanswered`.
- A **close**, as opposed to a crash, is different: "A close is not a crash: a request it stopped
  is sent again" (README L216; CHANGELOG L139 "A close still resends").
- `settings.restartLimit` (default 5, `agent.js` L36) bounds crash loops (types.d.ts L593–600).

Durable retries **are** visible to Forge's guard. Each attempt is a new `streamSimple` call
through `guardProvider`, which re-runs `authorization.admit()`.

### 3.3 What the log can and cannot say about delivery

- The checkpoint `phase: "request"` is committed before the call. Nothing is committed when
  bytes are written, so after a crash pi-durable cannot tell "never sent" from "sent, response
  lost". It treats both as a failed attempt (L125–127).
- For a returned error, the log holds only the pi-ai `AssistantMessage`, and the
  `errorMessage` string is the only failure detail. `status`, `headers`, `cause` and the
  `AggregateError`'s sub-errors are discarded in pi-ai (`openai-responses.js` L175–190).
- The provider session ID (`@earendil-works/pi-durable/dist/harness/provider.js` L17–29, one per
  conversation) goes out as `x-client-request-id` (`openai-responses.js` L219–229). It stays
  the same across attempts. Nothing in pi-ai, pi-durable or the SDK treats it as an
  idempotency or deduplication key, and this research did not establish any server-side
  deduplication.

## 4. Can a failure be shown not to have reached the provider?

Evidence chain for the observed `Connection error.: AggregateError (ETIMEDOUT)`, about 0.5 s
after start (`docs/release-verification/6a2196a.md` L78–79):

- Node 24.18.0 `lib/net.js` (embedded):
  - `internalConnectMultiple` L1178–1196: once every resolved address has been tried, the
    socket is destroyed with `new NodeAggregateError(context.errors)`. With zero errors it uses
    `ERR_SOCKET_CONNECTION_TIMEOUT` instead.
  - `internalConnectMultipleTimeout` L1801–1813: a per-address attempt that exceeds
    `autoSelectFamilyAttemptTimeout` pushes a `UV_ETIMEDOUT` connection error and tries the
    next address.
  - L149 and L263: the default attempt timeout is 250 ms (`net.getDefaultAutoSelectFamilyAttemptTimeout()`
    returns 250, and `getDefaultAutoSelectFamily()` returns `true`).
- `lib/internal/errors.js` L198–202: `NodeAggregateError` sets `code = errors[0]?.code`. So
  `(ETIMEDOUT)` is the **first** attempt's code. Later attempts' codes are not in the string.
- So an `AggregateError` from this path means the socket never left the `connecting` state, and
  no TCP connection was established to any address. TLS and the HTTP request are written only
  after connect, so no request bytes reached the provider. (Inference: about 0.5 s matches two
  250 ms attempts, for example one IPv6 and one IPv4 address.)
- No layer exposes this as a structured fact. The deepest-cause string is the only trace (§2.3).
  The same `Connection error.` prefix wraps every `fetch` rejection
  (`openai/client.js` L784–843), including post-write failures such as a dropped keep-alive
  socket (`other side closed`, `socket hang up`, `terminated`). The classifiers in §2.3 and §2.4
  treat all of these as retryable and do not separate pre-send from post-send.
- Failures after response headers (mid-stream) arrive from `processResponsesStream` and are
  never pre-send: the provider has accepted and started the request.

## 5. How a provider error reaches Missing Outcome in Forge

1. pi-ai pushes an `error` event with `stopReason: "error"` (§2.3). Forge's proxy redacts it and
   passes it on (`runner/src/profiles.ts` L121–129).
2. pi-durable `classify`: `settings.retry.enabled` is `false`, so it skips the retry. It appends
   the error assistant entry and ends the run `model_error`, and the input settles `unanswered`
   (`generation.js` L625–654; README L563).
3. The runner's `submission.wait()` resolves, and it emits `{ event: 'settled', status }`
   (`runner/src/check-runner.ts` L141, L205–206; `runner/src/recovery-consumer.ts` L136–137 for
   recovered sessions).
4. Host: `crates/server/src/durable_authorization/launch.rs` L1214 → `settled()` L1248–1273
   → `recovery::inspect_answer` → `Repository::observe_answer`.
5. `crates/store/src/durable_execution/completion.rs` L32–53: when the attempt has no accepted
   Outcome, it inserts the `missing-outcome` resource and a `dispatch_holds` row and returns
   `MissingOutcome`. The driver sets `Live(Blocked)` and keeps the lease
   (`launch.rs` L1265–1270). Capacity stays reserved (`docs/durable-execution.md` L387–393).

A durable-layer retry (layer 3) would therefore act at step 2, **before** the submission
settles. During the backoff the input stays pending, and Forge sees no `settled` event until
the retries finish or run out. A layer 2 retry acts inside step 1 and leaves no trace in the log.

## 6. Other packages checked

`@earendil-works/chord` 1.5.2 has no model-request retry. Its `retry` code
(`chord/dist/host/retry-on.js` and others) handles host object wake/activation, and chord's
`dist` does not import pi-ai. pi-durable imports only `@earendil-works/chord/json` in
`generation.js` (L1).
