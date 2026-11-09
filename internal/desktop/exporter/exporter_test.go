package exporter

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"image"
	"image/color"
	"image/png"
	"strings"
	"testing"
)

func TestExportRendering(t *testing.T) {
	t.Parallel()
	t.Run("offline assets and bridge isolation", func(t *testing.T) {
		body, err := decorate("<!doctype html><html><head></head><body>Answer</body></html>", "assets", PNG)
		if err != nil {
			t.Fatal(err)
		}
		for _, want := range []string{"script-src 'self'", "assets/bridge.js", "assets/html2canvas.js", "assets/capture.js", "data:image/svg+xml;base64,"} {
			if !strings.Contains(body, want) {
				t.Errorf("missing %s", want)
			}
		}
		for _, name := range []string{"bridge.js", "config.js", "mathjax.js", "fonts.js", "mermaid.js", "echarts.js", "prepare.js", "html2canvas.js", "capture.js", "static.js", "mathjax-LICENSE", "mathjax-font-LICENSE", "mermaid-LICENSE", "echarts-LICENSE", "echarts-NOTICE", "html2canvas-LICENSE"} {
			if data, err := resources.ReadFile("assets/" + name); err != nil || len(data) == 0 {
				t.Fatalf("missing asset %s: %v", name, err)
			}
		}
		bridge, _ := resources.ReadFile("assets/bridge.js")
		if strings.Contains(string(bridge), "window.mygo") || strings.Contains(string(bridge), "window.ipc") {
			t.Fatal("export page exposes a native bridge")
		}
	})
	t.Run("tiles preserve dimensions and pixels", func(t *testing.T) {
		tiles := []string{}
		for _, v := range []struct {
			height int
			color  color.Color
		}{{2048, color.RGBA{R: 255, A: 255}}, {1, color.RGBA{B: 255, A: 255}}} {
			canvas := image.NewRGBA(image.Rect(0, 0, 960, v.height))
			for y := 0; y < v.height; y++ {
				for x := 0; x < 960; x++ {
					canvas.Set(x, y, v.color)
				}
			}
			var data bytes.Buffer
			if err := png.Encode(&data, canvas); err != nil {
				t.Fatal(err)
			}
			tiles = append(tiles, "data:image/png;base64,"+base64.StdEncoding.EncodeToString(data.Bytes()))
		}
		body, _ := json.Marshal(map[string]any{"height": 2049, "tiles": tiles})
		result, err := decode(body, PNG)
		if err != nil {
			t.Fatal(err)
		}
		picture, err := png.Decode(bytes.NewReader(result))
		if err != nil {
			t.Fatal(err)
		}
		if picture.Bounds().Dx() != 960 || picture.Bounds().Dy() != 2049 {
			t.Fatal(picture.Bounds())
		}
		red, _, _, _ := picture.At(0, 2047).RGBA()
		_, _, blue, _ := picture.At(0, 2048).RGBA()
		if red != 65535 || blue != 65535 {
			t.Fatal("tile rows were lost")
		}
		body, _ = json.Marshal(map[string]any{"height": 2050, "tiles": tiles})
		if _, err = decode(body, PNG); err == nil {
			t.Fatal("invalid tile height was accepted")
		}
	})
	t.Run("standalone HTML and renderer failures", func(t *testing.T) {
		body, _ := json.Marshal(map[string]string{"html": "<!doctype html><html>Rendered</html>"})
		result, err := decode(body, HTML)
		if err != nil || !bytes.HasPrefix(result, []byte("<!doctype html>")) {
			t.Fatal(err)
		}
		for _, invalid := range []string{`{"html":"<html>missing doctype</html>"}`, `{"html":"<!doctype html>","extra":true}`, `{"error":"diagram failed"}`} {
			if _, err = decode([]byte(invalid), HTML); err == nil {
				t.Fatal("invalid document accepted")
			}
		}
	})
}
