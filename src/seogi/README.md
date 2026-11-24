# src/seogi

Seogi's own frontend lives here. Nothing in this folder imports from the
dictation settings screens under `src/components/`.

- `dev/` is a plain test screen for developers: record, bring a file in,
  watch the queue, read the minutes. It is not the product's UI and is meant
  to be replaced. Its texts are constants in `dev/text.ts`; the product UI
  goes through i18next like the rest of the app.

The UI talks to the backend only through the `seogi*` commands and the
`meetingChanged` / `recordingChanged` events in `src/bindings.ts` (generated
from `src-tauri/src/seogi/commands.rs` and `app.rs`). A new UI can be built
against those alone.
