# Tool content

An installable MCP plugin demonstrating Sailry's standard table, diff and image content. The execution Node needs Python 3; the server uses its standard library and performs no network requests or file mutations.

Zip this directory with `plugin.json` at the archive root, install it through the plugin manager, and select it for a conversation. Ask the Agent to call `report`. It returns the content in `content.json`, a small generated PNG, and a plain text summary. The same package runs on a local or remote Node without a desktop script.

The manifest captures `presentation: "content"` for this package's `reports/report` tool. MCP `structuredContent.sailry_content` contains the finite version 1 content. `image.index` references the original inline image position in the same result; it cannot name a path, URL, or another conversation's attachment.

The common native renderer supplies the table, diff, image viewer and expandable raw details. Model input, raw results and image bytes remain in authoritative conversation history. Disabling or removing the package does not remove existing results.

See [the SDK contract](../../../plugins/SDK.md#tool-message-content) for limits and fallback behavior. Existing progress operations and MCP elicitation use the native plan and question flows.
