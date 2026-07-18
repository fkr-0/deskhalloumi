# DeskHalloumi CopyQ Frontend API Spec

## Purpose

`deskhalloumi-copyq` is a fast, keyboard-friendly CopyQ clipboard history frontend. It is designed as a standalone DeskHalloumi binary so it can be used immediately without disturbing the existing status-bar and tray-window migration work.

Primary goals:

- fetch CopyQ history with one `copyq eval` call instead of one process per item
- render a searchable, bounded, keyboard-first UI for clipboard history
- preserve CopyQ history order during filtering
- support text, HTML, image, and non-text MIME entries safely
- show image clipboard entries as lazy-loaded thumbnails for visible rows
- support smooth open and close transitions without blocking history loading
- support single-entry activation and multi-entry merge workflows
- expose a small Rust API and a headless JSON mode for tests, launchers, and future integration

## CLI contract

```text
deskhalloumi-copyq [OPTIONS]
```

During development from the workspace:

```text
cargo run -p deskhalloumi-bin --bin deskhalloumi-copyq -- [OPTIONS]
```

Options:

| option | default | behavior |
| --- | --- | --- |
| `--copyq PATH` | `copyq` | CopyQ executable path. Useful for wrappers or tests. |
| `--max-preview-chars CHARS` | `220` | Maximum preview length per item. Long entries are middle-truncated with `…`. |
| `--max-visible-rows ROWS` | `160` | Maximum rows rendered after filtering. Keeps UI work bounded for large histories. |
| `--no-paste` | `false` | Select or merge items in CopyQ but do not paste into the focused application. |
| `--print-json` | `false` | Print normalized clipboard item metadata/previews as JSON and exit. |


## i3 operation contract

Recommended runtime command for a keyboard shortcut:

```sh
deskhalloumi-copyq --i3-shortcut
```

The `--i3-shortcut` mode adjusts the window for launcher-style operation:

- stable compatibility app id: `unilii-copyq`
- default window title: `DeskHalloumi CopyQ`
- centered popup window
- borderless client window
- always-on-top window level
- transparent/blur-capable background where supported
- non-resizable, non-minimizable window
- close-on-unfocus enabled
- close requests are intercepted so the same hide transition is used

The binary can print a starter i3 config snippet:

```sh
deskhalloumi-copyq --print-i3-config /path/to/deskhalloumi-copyq
```

Example i3 config:

```i3
for_window [app_id="unilii-copyq"] floating enable, border pixel 2, move position center, sticky enable
bindsym $mod+v exec --no-startup-id /path/to/deskhalloumi-copyq --i3-shortcut
```

On X11/i3, app-id matching may depend on the window backend. If the app-id rule does not match in a local setup, the stable title can be used as a fallback:

```i3
for_window [title="DeskHalloumi CopyQ"] floating enable, border pixel 2, move position center, sticky enable
```

The legacy app id is intentionally retained for the 0.2 compatibility cycle;
the command and visible title use DeskHalloumi branding. Changing the app id
without dual matching would silently break existing window-manager rules.

## UX contract

The UI is optimized for quick launcher-style use:

| interaction | behavior |
| --- | --- |
| type in search field | filter clipboard entries by all tokens, case-insensitive |
| `ArrowDown` / `Tab` | move selection down |
| `ArrowUp` | move selection up |
| `Enter` | activate selected entry |
| `Ctrl+R` | refresh CopyQ history |
| `Esc` | close window through the hide transition |
| `PageUp` / `PageDown` | jump selection by 10 entries |
| `Home` / `End` | jump to first / last visible entry |
| `Ctrl+U` | clear the query |
| `Ctrl+M` | merge visible entries |
| `F1` / `shortcuts` button | show or hide the complete shortcut guide |
| `refresh` button | reload history |
| `merge visible` button | merge exactly the currently rendered result window, add the result as a new CopyQ item, select it, then optionally paste |
| row `paste` button | activate that exact CopyQ index |

When filtered history is larger than `--max-visible-rows`, the rendered window
tracks the keyboard selection instead of leaving the selected row off-screen.
The footer reports the selected result, rendered row range, filtered count, and
total history count. Image preview loading follows the same rendered window.

When the shortcut guide is open, the first `Esc` hides the guide and a second
`Esc` closes the popup. This prevents an accidental close while consulting
keyboard help.

Rows include:

- visible result number
- type badge: `text`, `html`, `image`, `data`, or `mime`
- lazy image thumbnail for visible image rows when CopyQ exposes `image/*` data
- compact preview with newlines rendered as `↵`
- metadata line: CopyQ index, type, line count, char count, primary MIME type


## Animation contract

The standalone UI keeps a small visibility state machine:

```text
Opening -> Open -> Closing
```

- opening starts at animation progress `0.0` and eases to `1.0`
- closing eases down to `0.0`, then closes the window
- visual transition surfaces currently include panel alpha, content padding, list spacing, and row background alpha
- `Esc`, the close button, WM close requests, and successful paste activation use the same close transition
- i3 shortcut mode also closes through this transition on focus loss

## CopyQ IO contract

### History read

History read must be one external CopyQ command:

```text
copyq eval -- <history-script>
```

The script loops through `count()` inside CopyQ and prints a JSON array. It must not call `copyq read` externally per item.

Normalized item schema:

```json
[
  {
    "index": 0,
    "mime_type": "text/plain",
    "mime_types": ["text/plain"],
    "preview": "first line↵second line",
    "is_text": true,
    "image_mime_type": null,
    "chars": 22,
    "lines": 2
  }
]
```

Text handling:

- `text/plain` and `text/html` entries are read as text.
- `
`, `
`, and `
` are displayed as `↵` in previews.
- previews longer than `max_preview_chars` are middle-truncated.

Non-text handling:

- non-text entries are not decoded or embedded during the fast metadata pass
- preview becomes `<mime/type>` or `<image: image/type>`
- type badge is inferred from the MIME family where possible

Image handling:

- the metadata pass detects a preferred image MIME type from CopyQ's MIME list
- preferred order is PNG, JPEG, JPG, WebP, GIF, BMP, TIFF, then any other `image/*`
- image bytes are fetched lazily only for visible filtered rows using `copyq read <mime> <index>`
- image bytes are cached by CopyQ index until the history is refreshed
- failed or empty image reads render as compact thumbnail error placeholders

### Single activation

```text
copyq select <index>
copyq paste        # unless --no-paste was set
```

### Multi activation / merge

For multiple selected indices:

```text
copyq eval -- <selected-items-script>
copyq add <merged-output>
copyq select 0
copyq paste        # unless --no-paste was set
```

The selected-items script receives numeric indices generated by Rust, reads text or HTML entries verbatim, substitutes non-text entries with `<mime/type>`, and separates entries with a blank line.

## Rust API surface

Module:

```text
unilii/bin/src/copyq_frontend/mod.rs
```

Public types:

```rust
pub struct CopyqFrontendOptions {
    pub copyq_bin: String,
    pub max_preview_chars: usize,
    pub max_visible_rows: usize,
    pub paste_on_activate: bool,
    pub window_width: u32,
    pub window_height: u32,
    pub i3_shortcut_mode: bool,
    pub close_on_unfocus: bool,
    pub window_title: String,
    pub application_id: String,
}

pub struct ClipboardItem {
    pub index: usize,
    pub mime_type: String,
    pub mime_types: Vec<String>,
    pub preview: String,
    pub is_text: bool,
    pub image_mime_type: Option<String>,
    pub chars: usize,
    pub lines: usize,
}

pub enum CopyqError {
    CommandFailed { program: String, message: String },
    Json(String),
    EmptyHistory,
}

pub struct CopyqClient;
```

Public functions and methods:

```rust
impl CopyqClient {
    pub fn new(options: &CopyqFrontendOptions) -> Self;
    pub fn list_items(&self) -> Result<Vec<ClipboardItem>, CopyqError>;
    pub fn read_image_bytes(&self, index: usize, mime_type: &str) -> Result<Vec<u8>, CopyqError>;
    pub fn select_and_paste(&self, index: usize, paste: bool) -> Result<(), CopyqError>;
    pub fn merge_select_and_paste(&self, indices: &[usize], paste: bool) -> Result<(), CopyqError>;
}

pub fn run(options: CopyqFrontendOptions) -> iced::Result;
pub fn filter_item_indices(items: &[ClipboardItem], query: &str) -> Vec<usize>;
pub fn parse_clipboard_items_json(payload: &str) -> Result<Vec<ClipboardItem>, CopyqError>;
pub fn compact_preview(value: &str, max_chars: usize) -> String;
pub fn truncate_middle(value: &str, max_chars: usize) -> String;
pub fn apply_i3_shortcut_defaults(options: CopyqFrontendOptions) -> CopyqFrontendOptions;
pub fn build_window_settings(options: &CopyqFrontendOptions) -> iced::window::Settings;
pub fn i3_config_snippet(executable: &str, modifier: &str) -> String;
pub fn preferred_image_mime(mime_types: &[String]) -> Option<&str>;
pub fn build_history_eval_script(max_preview_chars: usize) -> String;
pub fn build_selected_items_eval_script(indices: &[usize]) -> String;
```

## Performance requirements

- history load: `O(n)` inside a single CopyQ process
- filtering: `O(n * tokens)` in memory
- rendering: bounded by `max_visible_rows`
- preview memory: bounded by `max_preview_chars` per displayed preview returned from CopyQ
- image memory: bounded to visible/cached image rows; cache is cleared on refresh
- no shell execution: commands are launched through `std::process::Command` with argv-style arguments

## Safety notes

- The frontend does not invoke `sh -c` for CopyQ operations.
- Multi-select script indices are `usize` values generated by Rust, not raw user strings.
- Non-text entries are displayed as MIME placeholders instead of decoded content during the metadata pass.
- Image bytes are only requested through CopyQ by numeric index and MIME type discovered from CopyQ metadata.
- Activation behavior is explicit: `--no-paste` keeps CopyQ selection updated without typing into the focused application.

## Test coverage

Focused unit tests cover:

- preview newline normalization and middle truncation
- JSON parsing for the CopyQ item schema
- query filtering while preserving CopyQ history order
- history script shape, including one `count()` loop and JSON output
- selected-items script shape using numeric index arrays
- image MIME preference and lazy visible-row image scheduling
- animation easing behavior
- i3 shortcut defaults and window settings
- keyboard selection bounds
- i3 config snippet generation
