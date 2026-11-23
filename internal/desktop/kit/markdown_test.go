package kit

import (
	"fmt"
	"strings"
	"testing"
	"time"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/uitest"
)

func TestMarkdownCharacterReveal(t *testing.T) {
	uitest.Parallel(t)
	for _, width := range []int{320, 960} {
		t.Run(fmt.Sprint(width), func(t *testing.T) {
			uitest.Parallel(t)
			src := ""
			animated, visible, revealed := true, true, false
			var options markdown.Options
			tt := uitest.New(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				ui.Column(c).Fill().Children(func() {
					if visible {
						revealed = markdown.Revealed(Markdown(c, src, MarkdownProps{Variant: Chat, Animated: animated, Options: &options}))
					}
				})
			}, width, 600, uitest.Scale, "", ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
			src = strings.Repeat("abcdefghij", 20) + "\u4e2d\u6587\U0001f331"
			tt.Frame()
			tt.Advance(20 * time.Millisecond)
			first := strings.Join(tt.Texts(), "")
			if len(first) == 0 || len(first) >= len(src) || !strings.HasPrefix(src, first) || !utf8.ValidString(first) || revealed {
				t.Fatalf("the first burst was not revealed by character: %q, revealed %v", first, revealed)
			}
			fadingWhole := false
			for range 150 {
				tt.Advance(16 * time.Millisecond)
				shown := strings.Join(tt.Texts(), "")
				fadingWhole = fadingWhole || shown == src && !revealed
				if !strings.HasPrefix(src, shown) || !utf8.ValidString(shown) {
					t.Fatalf("invalid character boundary while revealing: %q", shown)
				}
				if revealed {
					break
				}
			}
			if !fadingWhole || !revealed || strings.Join(tt.Texts(), "") != src || tt.FramePending() {
				t.Fatal("the stream did not finish its fade and stop requesting frames")
			}
			src += strings.Repeat("append", 40)
			tt.Frame()
			if revealed {
				t.Fatal("a new burst did not restart reveal")
			}
			animated = false
			tt.Frame()
			if !revealed || strings.Join(tt.Texts(), "") != src || tt.FramePending() {
				t.Fatal("completion did not flush pending characters and fade")
			}
			for range 3 {
				visible = false
				tt.Frame()
				visible = true
				tt.Frame()
				if !revealed || strings.Join(tt.Texts(), "") != src || tt.FramePending() {
					t.Fatal("reopening history replayed the stream")
				}
			}
		})
	}
}

func TestMarkdownRevealBudget(t *testing.T) {
	uitest.Parallel(t)
	src := ""
	var options markdown.Options
	var revealed bool
	var layouts func() uint64
	tt := uitest.New(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		revealed = markdown.Revealed(Markdown(c, src, MarkdownProps{Variant: Chat, Animated: true, Options: &options}))
		layouts = c.TextSystem().LayoutsMade
	}, 600, 400, uitest.Scale, "", ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	src = strings.Repeat("x", streamRevealLimit-1)
	tt.Frame()
	tt.Advance(20 * time.Millisecond)
	if shown := strings.Join(tt.Texts(), ""); revealed || shown == "" || !strings.HasPrefix(src, shown) {
		t.Fatal("a short source did not retain character reveal")
	}
	for range 8 {
		src += "x"
		tt.Frame()
		if !revealed || !tt.HasText(src) || tt.FramePending() {
			t.Fatal("a source at the reveal budget did not flush and stay on the caller's cadence")
		}
		before := layouts()
		tt.Advance(300 * time.Millisecond)
		tt.Frame()
		if layouts() != before {
			t.Fatalf("an unchanged long source made additional layouts: %d", layouts()-before)
		}
	}
	src += "\n\nA short tail"
	tt.Frame()
	if !revealed || !tt.HasText("A short tail") || tt.FramePending() {
		t.Fatal("a short tail restarted reveal over a long source prefix")
	}
	src = "A new short source"
	tt.Frame()
	src += strings.Repeat(" appended", 20)
	tt.Frame()
	if revealed {
		t.Fatal("replacing a long source permanently disabled character reveal")
	}
}

func TestMarkdownReducedMotion(t *testing.T) {
	uitest.Parallel(t)
	src := ""
	var options markdown.Options
	var revealed bool
	tt := uitest.New(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		revealed = markdown.Revealed(Markdown(c, src, MarkdownProps{Animated: true, Options: &options}))
	}, 600, 400, uitest.Scale, "", ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	src = strings.Repeat("streaming text ", 30) + "end"
	tt.Frame()
	if revealed {
		t.Fatal("streaming text did not animate before reduced motion was enabled")
	}
	tt.SetPreferences(ui.Preferences{ReduceMotion: true, TextScale: 1})
	tt.Frame()
	if !revealed || !tt.HasText(src) || tt.FramePending() {
		t.Fatal("reduced motion did not immediately flush the active reveal")
	}
	src += "more text"
	tt.Frame()
	if !revealed || !tt.HasText(src) || tt.FramePending() {
		t.Fatal("reduced motion paced new text")
	}
	tt.SetPreferences(ui.Preferences{TextScale: 1})
	tt.Frame()
	if !revealed {
		t.Fatal("changing motion preferences replayed existing text")
	}
	src += strings.Repeat(" new text", 30)
	tt.Frame()
	if revealed {
		t.Fatal("motion preference changes permanently disabled future reveal")
	}
}

func TestMarkdownPreservesExplicitReveal(t *testing.T) {
	uitest.Parallel(t)
	reveal := &markdown.Reveal{Unit: markdown.ByLine, Fade: -1}
	options := markdown.Options{Reveal: reveal}
	tt := uitest.New(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		Markdown(c, "An unfinished line", MarkdownProps{Animated: true, Options: &options})
	}, 600, 400, uitest.Scale, "", ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	if options.Reveal != reveal || tt.HasText("An unfinished line") {
		t.Fatal("Kit replaced the caller's explicit reveal policy")
	}
}

func TestMarkdownStreamingFence(t *testing.T) {
	uitest.Parallel(t)
	for _, tc := range []struct {
		width    int
		complete bool
	}{{320, false}, {960, false}, {320, true}, {960, true}} {
		t.Run(fmt.Sprintf("%d/complete=%v", tc.width, tc.complete), func(t *testing.T) {
			uitest.Parallel(t)
			src := ""
			animated := true
			var options markdown.Options
			var revealed bool
			tt := uitest.New(func(c *ui.Context) {
				theme.Apply(c, "", theme.Dark)
				ui.Column(c).Fill().Children(func() {
					revealed = markdown.Revealed(Markdown(c, src, MarkdownProps{Variant: Chat, Animated: animated, Options: &options}))
				})
				ToastHost(c)
			}, tc.width, 600, uitest.Scale, "en", ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
			const code = "func main() {\n\tfmt.Println(\"streaming\")\n}\n"
			src = "```go\n" + code
			tt.Frame()
			if tt.HasText(strings.TrimSuffix(code, "\n")) {
				t.Fatal("the code fence skipped native character pacing")
			}
			settle := func() {
				t.Helper()
				for range 150 {
					tt.Advance(16 * time.Millisecond)
					if revealed {
						return
					}
				}
				t.Fatal("the code fence never finished revealing")
			}
			settle()
			copyLabel := i18n.In("en", "content_copy")
			button, ok := tt.AccessByRole(ui.RoleButton, copyLabel)
			if !ok || !button.Disabled || !tt.HasText(strings.TrimSuffix(code, "\n")) {
				t.Fatal("the open fence lost its body or enabled copying before it closed")
			}
			if _, ok := tt.AccessByRole(ui.RoleTextField, "Code editor"); ok {
				t.Fatal("the streaming fence rebuilt a separate editor instead of using native Markdown chunks")
			}
			src += "```"
			tt.Frame()
			settle()
			button, ok = tt.AccessByRole(ui.RoleButton, copyLabel)
			if !ok || !button.Disabled {
				t.Fatal("the pending closing markers enabled copying before the line or stream ended")
			}
			if tc.complete {
				animated = false
			} else {
				src += "\n"
			}
			tt.Frame()
			settle()
			button, ok = tt.AccessByRole(ui.RoleButton, copyLabel)
			if !ok || button.Disabled || button.Bounds.X < 0 || button.Bounds.X+button.Bounds.W > float32(tc.width) {
				t.Fatalf("the closed fence has no accessible in-bounds copy action: found %v, button %+v, texts %q", ok, button, tt.Texts())
			}
			tt.Key(0, ui.KeyTab)
			tt.Frame()
			button, _ = tt.AccessByRole(ui.RoleButton, copyLabel)
			if !button.Focused {
				t.Fatal("the fence copy action is not reachable with Tab")
			}
			tt.Key(0, ui.KeyEnter)
			tt.Frame()
			if tt.Clipboard() != code || !tt.HasText(i18n.In("en", "content_copied")) {
				t.Fatal("keyboard copy did not preserve code and its confirmation toast")
			}
		})
	}
}
