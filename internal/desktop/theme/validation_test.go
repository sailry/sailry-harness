package theme

import (
	"bytes"
	"encoding/json"
	"fmt"
	"image"
	"image/png"
	"io/fs"
	"strings"
	"testing"
	"testing/fstest"
)

// The manifest mirrors the CC0 Tide source fixture; artwork stays in memory.
func themeFixture(t *testing.T) (map[string]any, fstest.MapFS) {
	t.Helper()
	var encoded bytes.Buffer
	if err := png.Encode(&encoded, image.NewRGBA(image.Rect(0, 0, 2, 2))); err != nil {
		t.Fatal(err)
	}
	files := fstest.MapFS{"assets/light.png": {Data: encoded.Bytes()}, "assets/dark.png": {Data: encoded.Bytes()}}
	variant := func(mode string) map[string]any {
		assets := map[string]any{}
		for slot := range themeSlots {
			height := 36
			if slot == "hero.new_session" {
				height = 100
			}
			assets[slot] = map[string]any{"path": "assets/" + mode + ".png", "fit": "cover", "opacity": 0.8, "alignment": []any{0.0, 0.5}, "height": height}
		}
		return map[string]any{"theme": map[string]any{"mode": mode, "colors": map[string]any{"background": "#edf5fb", "group_box.background": "#e0eef5"}}, "assets": assets,
			"composer": map[string]any{"send": map[string]any{"background": "#fff", "foreground": "#000", "hover": "#ddd", "active": "#ccc"}},
			"cards":    map[string]any{"explore": map[string]any{"border_width": 1, "radius": 16}},
			"messages": map[string]any{"user": map[string]any{"background": "#eee", "border_width": 1}}, "wordmark": map[string]any{"foreground": "#aaa"}}
	}
	return map[string]any{"version": 1, "id": "tide", "name": "Tide", "description": "A coastal theme", "package_version": "0.1.0", "license": map[string]any{"author": "Sailry", "name": "CC0-1.0", "text": "Original test artwork, dedicated to the public domain under CC0 1.0"}, "light": variant("light"), "dark": variant("dark")}, files
}

func setThemeField(value map[string]any, pointer string, replacement any) {
	parts := strings.Split(strings.TrimPrefix(pointer, "/"), "/")
	object := value
	for _, part := range parts[:len(parts)-1] {
		object = object[part].(map[string]any)
	}
	object[parts[len(parts)-1]] = replacement
}

func TestThemeInvalidFixtures(t *testing.T) {
	t.Parallel()
	layer := func() map[string]any {
		return map[string]any{"kind": "twinkle", "color": "#80dfff", "count": 12, "opacity": 0.2, "size": 2, "duration": 12}
	}
	rows := []struct {
		pointer     string
		replacement any
		reason      string
	}{
		{"/version", 2, "unsupported theme package version"},
		{"/id", "../outside", "invalid theme package ID"}, {"/id", "builtin", "invalid theme package ID"},
		{"/name", " ", "invalid theme package name"},
		{"/description", " \n\t", "invalid theme package description"}, {"/description", strings.Repeat("d", 4097), "invalid theme package description"}, {"/description", "invalid\x00", "invalid theme package description"},
		{"/package_version", " ", "invalid theme release version"}, {"/package_version", strings.Repeat("v", 65), "invalid theme release version"}, {"/package_version", "0.1.0\n", "invalid theme release version"},
		{"/homepage", "https://user:secret@example.invalid/", "invalid theme homepage"}, {"/homepage", "file:///tmp/theme", "invalid theme homepage"},
		{"/license/text", "", "theme package requires author and license text"}, {"/license/author", "\t", "theme package requires author and license text"},
		{"/license/name", " ", "invalid theme license name"}, {"/license/name", strings.Repeat("l", 161), "invalid theme license name"},
		{"/dark/theme/mode", "light", "theme package requires light and dark variants"},
		{"/light/theme/colors/background", "not-a-color", "Unsupported background value"},
		{"/light/theme/colors/background", "https://example.invalid/theme.png", "Unsupported background value"},
		{"/light/theme/colors/unknown", "#fff", "unknown Kit color: unknown"},
		{"/light/theme/highlight", map[string]any{}, "unsupported theme field: highlight"},
		{"/light/theme/opacity", 0.5, "unsupported theme field: opacity"},
		{"/light/theme/font.size", 33, "invalid theme sizing"}, {"/light/theme/radius", 33, "invalid theme sizing"},
		{"/light/assets/brand/path", "assets/../secret.png", "theme resource must be a normalized path under assets/"},
		{"/light/assets/brand/path", "/tmp/secret.png", "theme resource must be a normalized path under assets/"},
		{"/light/assets/brand/path", "assets\\secret.png", "theme resource must be a normalized path under assets/"},
		{"/light/assets/brand/path", "https://example.invalid/image.png", "theme resource must be a normalized path under assets/"},
		{"/light/assets/brand/path", "assets/script.svg", "unsupported theme image format"},
		{"/light/assets/brand/opacity", 2, "invalid theme decoration dimensions: brand"},
		{"/light/assets/brand/height", 10000, "invalid theme decoration dimensions: brand"},
		{"/light/assets/brand/alignment", []any{0, 0, 0}, "expected array of 2 elements"},
		{"/light/assets/brand/fit", "unknown", "unknown theme fit: unknown"},
		{"/light/assets/brand/tile_size", 0, "invalid theme tiling: brand"},
		{"/light/assets/background.content/tile_size", 1025, "invalid theme tiling: background.content"},
		{"/light/assets/icon.send/fit", "tile", "invalid theme tiling: icon.send"},
		{"/light/assets/unknown", map[string]any{"path": "assets/light.png"}, "unsupported theme slot: unknown"},
		{"/light/composer/border_width", 5, "invalid composer border width"},
		{"/light/composer/send/background", "not-a-color", "Invalid color name"},
		{"/light/composer/send/hover", "linear-gradient(black, red)", "Invalid color name"},
		{"/light/cards/explore/border_width", 5, "invalid welcome card dimensions: explore"},
		{"/light/cards/explore/radius", 33, "invalid welcome card dimensions: explore"},
		{"/light/cards/explore/background", "linear-gradient(black, red)", "Invalid color name"},
		{"/light/cards", map[string]any{"other": map[string]any{}}, "unsupported welcome card: other"},
		{"/light/messages/user/border_width", 5, "invalid message dimensions: user"},
		{"/light/messages/user/radius", 33, "invalid message dimensions: user"},
		{"/light/messages", map[string]any{"system": map[string]any{}}, "unsupported message style: system"},
		{"/light/wordmark/glow", "invalid", "Invalid color name"},
		{"/light/unknown", true, "unknown field"}, {"/unknown", true, "unknown field"},
	}
	for _, row := range []struct{ pointer, token string }{
		{"/light/theme/radius", "8.0"}, {"/light/theme/radius.lg", "8.0"}, {"/light/composer/border_width", "1.0"}, {"/light/assets/brand/height", "64.0"}, {"/light/assets/brand/tile_size", "256.0"},
	} {
		rows = append(rows, struct {
			pointer     string
			replacement any
			reason      string
		}{row.pointer, json.Number(row.token), "cannot unmarshal number"})
	}
	for _, target := range []string{"button.send", "background.card.plan", "background.unknown", "script"} {
		rows = append(rows, struct {
			pointer     string
			replacement any
			reason      string
		}{"/light/effects", map[string]any{target: []any{layer()}}, "invalid theme effect target: " + target})
	}
	for key, replacement := range map[string]any{"count": 0, "opacity": 1.1, "size": 13, "duration": 121, "kind": "pulse"} {
		invalid := layer()
		invalid[key] = replacement
		rows = append(rows, struct {
			pointer     string
			replacement any
			reason      string
		}{"/light/effects", map[string]any{"conversation": []any{invalid}}, "invalid theme effect dimensions: conversation"})
	}
	for index, row := range rows {
		t.Run(row.pointer+"/"+fmt.Sprint(index), func(t *testing.T) {
			value, files := themeFixture(t)
			setThemeField(value, row.pointer, row.replacement)
			data, err := json.Marshal(value)
			if err != nil {
				t.Fatal(err)
			}
			if _, err = Parse(data, files); err == nil || !strings.Contains(err.Error(), row.reason) {
				t.Fatalf("got %v; want reason %q", err, row.reason)
			}
		})
	}
	t.Run("optional metadata and colors", func(t *testing.T) {
		value, files := themeFixture(t)
		for _, pointer := range []string{"/description", "/package_version", "/license/name", "/light/theme/colors/background", "/light/messages/user/background", "/light/wordmark/foreground"} {
			setThemeField(value, pointer, nil)
		}
		setThemeField(value, "/light/theme/colors/group_box.background", "blue-600/33")
		data, _ := json.Marshal(value)
		if _, err := Parse(data, files); err != nil {
			t.Fatal(err)
		}
	})
	t.Run("resource limits", func(t *testing.T) {
		for _, scenario := range []string{"missing", "invalid", "oversized", "dimensions", "symlink", "manifest", "16-bit decoder allocation"} {
			t.Run(scenario, func(t *testing.T) {
				value, files := themeFixture(t)
				data, _ := json.Marshal(value)
				switch scenario {
				case "missing":
					delete(files, "assets/light.png")
				case "invalid":
					files["assets/light.png"].Data = []byte("not an image")
				case "oversized":
					files["assets/light.png"].Data = make([]byte, themeFileLimit+1)
				case "dimensions":
					var encoded bytes.Buffer
					png.Encode(&encoded, image.NewRGBA(image.Rect(0, 0, 4097, 1)))
					files["assets/light.png"].Data = encoded.Bytes()
				case "symlink":
					files["assets/light.png"].Mode = fs.ModeSymlink
				case "16-bit decoder allocation":
					var encoded bytes.Buffer
					if err := png.Encode(&encoded, image.NewNRGBA64(image.Rect(0, 0, 4096, 2048))); err != nil {
						t.Fatal(err)
					}
					files["assets/light.png"].Data = encoded.Bytes()
				case "manifest":
					data = bytes.Repeat([]byte(" "), themeManifestLimit+1)
				}
				if _, err := Parse(data, files); err == nil {
					t.Fatal("invalid resource accepted")
				} else if scenario == "16-bit decoder allocation" && !strings.Contains(err.Error(), "theme image exceeds decoded resource limit") {
					t.Fatalf("decoder allocation was not bounded before decoding: %v", err)
				}
			})
		}
	})
	t.Run("effect budgets", func(t *testing.T) {
		for _, count := range []int{0, 4, 5} {
			value, files := themeFixture(t)
			layers := []any{}
			for range count {
				effect := layer()
				effect["count"] = 64
				layers = append(layers, effect)
			}
			setThemeField(value, "/light/effects", map[string]any{"conversation": layers})
			data, _ := json.Marshal(value)
			if _, err := Parse(data, files); err == nil {
				t.Fatalf("effect layer budget accepted %d", count)
			}
		}
		value, files := themeFixture(t)
		setThemeField(value, "/light/effects", map[string]any{"hero.new_session": []any{layer()}})
		delete(value["light"].(map[string]any)["assets"].(map[string]any), "hero.new_session")
		data, _ := json.Marshal(value)
		if _, err := Parse(data, files); err == nil || !strings.Contains(err.Error(), "requires artwork") {
			t.Fatalf("missing effect artwork: %v", err)
		}
	})
}
