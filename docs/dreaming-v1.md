# Dreaming v1 (experimental)

Branch: `feature/rusty-dreaming-v1`. No cloud backend or deployment required.

## What works

- Opt-in separate model endpoint in **Settings → Memoria**, with LM Studio, Ollama,
  Gemini presets and any OpenAI-compatible API URL.
- Remote conversation transfer requires an explicit separate checkbox; remote
  endpoints must use HTTPS. A local endpoint does not require an API key.
- Every two minutes (when no conversational request is pending), or manually,
  process up to 8 unprocessed text messages (max 2,000 characters each) using the selected Dreaming model.
- Persist short model-proposed facts and a processing checkpoint in SQLite.
  Checkpoint advances only after valid JSON and a committed transaction.
- Retrieve up to five keyword-related memories and add them as **untrusted data**
  in the conversational model context, never as executable instructions.
- List and delete saved memories in the settings screen.

## Deliberately deferred

- Vector similarity/embeddings: v1 uses deterministic keyword matching, with no
  second model or additional database dependency.
- Multi-device synchronization and automatic merging/conflict resolution.
- A separate user-editable memory editor, deduplication of semantically equivalent
  facts, memory contradiction resolution, automatic summarization of long histories.

## Privacy / limitations

- The memory extractor is allowed to see **conversation text** only, never tool
  result payloads or system prompts. The prompt instructs it to omit secrets,
  but no automated classifier can guarantee that: **do not enable a remote model
  for sensitive history**.
- This feature defaults to **disabled**. Changing the model does not erase memories.
- Memory is extracted from conversations, not external OS events. It does not
  modify the animation event bus.
- If the model returns malformed JSON or the connection fails, the batch remains
  pending; manual retries are possible. A later batch processes the next messages.
- OpenAI-compatible Dreaming requests wait up to 180 seconds to accommodate
  local inference on modest hardware; the normal chat timeout remains 30 seconds.
- Deleting a conversation does not currently cascade into extracted memories;
  delete unwanted memories explicitly in Settings → Memoria.
- Models may be instructed to output `{"memories":[]}` when nothing is worth saving.

## LM Studio quick check

1. Enable LM Studio local server and load a chat/instruct model.
2. Settings → Memoria: enable, set `http://localhost:1234/v1` and the actual model ID.
3. Save configuration and click **Procesar historial ahora**.
4. Verify extracted memories and ask the conversational model about a topic
   with a matching keyword to confirm retrieval.

No changes are made to the Nueva Farma project.
