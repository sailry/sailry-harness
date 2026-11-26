package kit

import (
	"runtime"
	"testing"
	"time"
	"weak"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestRetiredThemeCachesReleaseAfterGC(t *testing.T) {
	var mdKey markdownThemeKey
	var codeKey codeThemeKey
	var thWeak weak.Pointer[markdown.Theme]

	func() {
		p := &theme.Palette{
			Background:      ui.RGB(10, 20, 30),
			Foreground:      ui.RGB(230, 230, 230),
			Muted:           ui.RGB(40, 40, 40),
			MutedForeground: ui.RGB(140, 140, 140),
			Primary:         ui.RGB(90, 150, 240),
			Link:            ui.RGB(90, 150, 240),
			Border:          ui.RGB(60, 60, 60),
			Selection:       ui.RGB(70, 90, 120),
			Caret:           ui.RGB(255, 255, 255),
			Success:         ui.RGB(50, 200, 100),
			Danger:          ui.RGB(220, 80, 80),
			Warning:         ui.RGB(230, 180, 60),
		}
		mdKey = markdownThemeKey{weak.Make(p), Chat}
		codeKey = codeThemeKey{weak.Make(p), lookPlain}

		th := markdownThemeForPalette(p, Chat, func() *markdown.Theme { return &markdown.Theme{} })
		if !isMarkdownTheme(th) {
			t.Fatal("expected Markdown theme to be recognized as Kit-owned")
		}
		thWeak = weak.Make(th)

		_ = codeThemeForPalette(p, lookPlain, func() *editor.Theme { return &editor.Theme{} })

		if _, ok := markdownThemes.Load(mdKey); !ok {
			t.Fatal("expected markdownThemes entry while palette is live")
		}
		if _, ok := codeThemes.Load(codeKey); !ok {
			t.Fatal("expected codeThemes entry while palette is live")
		}
		runtime.KeepAlive(p)
		runtime.KeepAlive(th)
	}()

	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		runtime.GC()
		runtime.Gosched()
		_, mdCached := markdownThemes.Load(mdKey)
		_, codeCached := codeThemes.Load(codeKey)
		_, ownCached := markdownOwn.Load(thWeak)
		if !mdCached && !codeCached && !ownCached {
			return
		}
		time.Sleep(5 * time.Millisecond)
	}
	_, mdCached := markdownThemes.Load(mdKey)
	_, codeCached := codeThemes.Load(codeKey)
	_, ownCached := markdownOwn.Load(thWeak)
	t.Fatalf("retired theme cache entries remained after GC: markdownThemes=%t codeThemes=%t markdownOwn=%t", mdCached, codeCached, ownCached)
}
