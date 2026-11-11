package node

import (
	"bytes"
	"image"
	"image/png"
	"math/rand/v2"
	"strings"
	"testing"

	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
)

// TestToolImageBounds covers the images an MCP server returns, such as a
// computer-use screenshot of a whole display.
func TestToolImageBounds(t *testing.T) {
	encode := func(width, height int, noise bool) []byte {
		picture := image.NewNRGBA(image.Rect(0, 0, width, height))
		if noise {
			source := rand.New(rand.NewPCG(1, 2))
			for index := range picture.Pix {
				picture.Pix[index] = byte(source.UintN(256))
				if index%4 == 3 {
					picture.Pix[index] = 255
				}
			}
		}
		var encoded bytes.Buffer
		if err := png.Encode(&encoded, picture); err != nil {
			t.Fatal(err)
		}
		return encoded.Bytes()
	}
	small, display := encode(64, 48, false), encode(2880, 1800, true)
	if len(display) <= 5*1024*1024 {
		t.Fatalf("the display capture is only %d bytes", len(display))
	}
	result := &sdk.CallToolResult{Content: []sdk.Content{
		&sdk.TextContent{Text: "captured"},
		&sdk.ImageContent{Data: small, MIMEType: "image/png"},
		&sdk.ImageContent{Data: display, MIMEType: "image/png"},
		&sdk.ImageContent{Data: []byte("<svg/>"), MIMEType: "image/svg+xml"},
	}}
	boundToolImages(result)
	if text, ok := result.Content[0].(*sdk.TextContent); !ok || text.Text != "captured" || len(result.Content) != 4 {
		t.Fatalf("text content = %+v of %d parts", result.Content[0], len(result.Content))
	}
	if kept := result.Content[1].(*sdk.ImageContent); !bytes.Equal(kept.Data, small) || kept.MIMEType != "image/png" {
		t.Fatalf("an image within the limits was changed: %s, %d bytes", kept.MIMEType, len(kept.Data))
	}
	reduced := result.Content[2].(*sdk.ImageContent)
	config, format, err := image.DecodeConfig(bytes.NewReader(reduced.Data))
	if err != nil || reduced.MIMEType != "image/"+format || config.Width > 1568 || config.Width < 1560 || len(reduced.Data) > 3*1024*1024 {
		t.Fatalf("display capture = %s %dx%d in %d bytes, %v", reduced.MIMEType, config.Width, config.Height, len(reduced.Data), err)
	}
	if config.Width*1800/2880 != config.Height {
		t.Fatalf("display capture lost its shape: %dx%d", config.Width, config.Height)
	}
	if note, ok := result.Content[3].(*sdk.TextContent); !ok || !strings.HasPrefix(note.Text, "image omitted: ") {
		t.Fatalf("an image no model accepts = %+v", result.Content[3])
	}
}
