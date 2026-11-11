package node

import (
	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/node/files"
)

// boundToolImages gives the images an MCP server returned, a computer-use
// screenshot above all, the limits of every other image a tool returns: at
// most 1568 pixels on a side and 3 MiB. A server sends a capture at the size
// of the screen, which no model accepts. An image that cannot be reduced is
// replaced by a note that says so.
func boundToolImages(result *sdk.CallToolResult) {
	for index, content := range result.Content {
		image, ok := content.(*sdk.ImageContent)
		if !ok {
			continue
		}
		source, sent, data, err := files.PrepareCapture(image.Data)
		if err != nil {
			result.Content[index] = &sdk.TextContent{Text: "image omitted: " + err.Error()}
			continue
		}
		image.Data, image.MIMEType = data, source.MimeType
		if sent != nil {
			image.MIMEType = sent.MimeType
		}
	}
}
