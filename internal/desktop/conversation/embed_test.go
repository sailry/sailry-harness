package conversation_test

import (
	"bytes"
	"image"
	"image/png"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func testEmbedBitmap(t *testing.T) ui.ImageSource {
	t.Helper()
	var buf bytes.Buffer
	if err := png.Encode(&buf, image.NewNRGBA(image.Rect(0, 0, 16, 12))); err != nil {
		t.Fatal(err)
	}
	bmp, err := ui.DecodeBitmap(buf.Bytes())
	if err != nil {
		t.Fatal(err)
	}
	return bmp
}

func TestEmbedPassesAltAndHandlesLinkedAndMixedImages(t *testing.T) {
	bmp := testEmbedBitmap(t)
	var seen []struct {
		dest, label string
		image       bool
	}
	var opened []string
	s := conversation.New()
	s.OpenLink = func(_ *ui.Context, _, link string) bool {
		opened = append(opened, link)
		return true
	}
	s.Embed = func(_, dest, label string, img bool) (conversation.Embed, bool) {
		seen = append(seen, struct {
			dest, label string
			image       bool
		}{dest, label, img})
		switch dest {
		case "https://cdn.example.test/home.webp", "https://cdn.example.test/poster.png":
			return conversation.Embed{Image: &conversation.Image{ID: "remote/" + dest, Name: label, Width: 16, Height: 12, Source: bmp}}, true
		case "https://cdn.example.test/broken.png":
			return conversation.Embed{}, true
		default:
			return conversation.Embed{}, false
		}
	}
	turns := []conversation.Turn{answered("t", "Show remote images",
		"![带 macOS 壁纸的 Sailry 桌面首页](https://cdn.example.test/home.webp)\n\n"+
			"[![失败的链接图](https://cdn.example.test/broken.png)](https://github.com/sailry/sailry-code)\n\n"+
			"这是刚生成的配图：\n![海报](https://cdn.example.test/poster.png)\n详见 [仓库](https://example.test/repo)。")}

	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() { conversation.View(c, s, "session", turns, nil, noActions{}) })
	}, 900, 1000)
	tt.SetLocale("zh-CN")
	tt.Frame()
	tt.Frame()

	for _, alt := range []string{"带 macOS 壁纸的 Sailry 桌面首页", "海报"} {
		if _, ok := tt.Find(alt); !ok {
			if _, ok := tt.AccessByRole(ui.RoleButton, alt); !ok {
				t.Fatalf("embedded remote image %q did not render: texts=%q", alt, tt.Texts())
			}
		}
	}
	if !tt.HasText("这是刚生成的配图：") || !tt.HasText("仓库") {
		t.Fatalf("mixed paragraph text/link was lost: %q", tt.Texts())
	}
	// Linked image fallback opens its wrapping link destination.
	if err := tt.Click("https://github.com/sailry/sailry-code"); err != nil {
		t.Fatalf("clicking linked image fallback: %v (texts=%q)", err, tt.Texts())
	}
	tt.Frame()
	if !slices.Equal(opened, []string{"https://github.com/sailry/sailry-code"}) {
		t.Fatalf("opened links = %q", opened)
	}
	// Clicking the inline link in the mixed paragraph still works.
	link, ok := tt.AccessByRole(ui.RoleLink, "仓库")
	if !ok {
		t.Fatalf("inline link in mixed paragraph lost role: %q", tt.Texts())
	}
	tt.ClickAt(link.Bounds.X+link.Bounds.W/2, link.Bounds.Y+link.Bounds.H/2)
	tt.Frame()
	if !slices.Equal(opened, []string{"https://github.com/sailry/sailry-code", "https://example.test/repo"}) {
		t.Fatalf("opened links after mixed text link = %q", opened)
	}
	// Alt text was forwarded to Embed resolver.
	foundAlt := false
	for _, call := range seen {
		if call.dest == "https://cdn.example.test/home.webp" && call.label == "带 macOS 壁纸的 Sailry 桌面首页" && call.image {
			foundAlt = true
		}
	}
	if !foundAlt {
		t.Fatalf("alt label was not forwarded to Embed: %+v", seen)
	}
}

func TestEmbedLinkedImageKeepsDetailAndMixedRunsStayText(t *testing.T) {
	bmp := testEmbedBitmap(t)
	var opened []string
	s := conversation.New()
	s.OpenLink = func(_ *ui.Context, _, link string) bool {
		opened = append(opened, link)
		return true
	}
	s.Embed = func(_, dest, label string, img bool) (conversation.Embed, bool) {
		if img && (dest == "https://cdn.example.test/poster.png" || dest == "https://cdn.example.test/home.webp") {
			return conversation.Embed{Image: &conversation.Image{ID: "remote/" + dest, Name: label, Width: 16, Height: 12, Source: bmp}}, true
		}
		return conversation.Embed{}, false
	}
	turns := []conversation.Turn{answered("t", "Show linked images",
		"[![详情海报](https://cdn.example.test/poster.png)](https://example.test/detail)\n\n"+
			"说明 ![首页](https://cdn.example.test/home.webp) # 不是标题\n2. 不是列表 ![海报二](https://cdn.example.test/poster.png) ---")}

	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() { conversation.View(c, s, "session", turns, nil, noActions{}) })
	}, 900, 1000)
	tt.SetLocale("zh-CN")
	tt.Frame()
	tt.Frame()

	for _, alt := range []string{"详情海报", "首页", "海报二"} {
		if _, ok := tt.Find(alt); !ok {
			if _, ok := tt.AccessByRole(ui.RoleButton, alt); !ok {
				t.Fatalf("embedded image %q did not render: texts=%q", alt, tt.Texts())
			}
		}
	}
	// A displayed linked picture keeps its detail link.
	if err := tt.Click("https://example.test/detail"); err != nil {
		t.Fatalf("linked image detail link missing: %v (texts=%q)", err, tt.Texts())
	}
	tt.Frame()
	if !slices.Equal(opened, []string{"https://example.test/detail"}) {
		t.Fatalf("opened links = %q", opened)
	}
	// Text cut around pictures keeps its literal markers.
	for _, text := range []string{"# 不是标题", "2. 不是列表", "---"} {
		if !tt.HasText(text) {
			t.Fatalf("mixed run %q was reinterpreted: texts=%q", text, tt.Texts())
		}
	}
	if _, ok := tt.AccessByRole(ui.RoleHeading, "不是标题"); ok {
		t.Fatal("mixed run became a heading")
	}
}
