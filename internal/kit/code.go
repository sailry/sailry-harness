package kit

import (
	"strings"
	"sync"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/sailry/sailry-go/internal/theme"
)

// Code and diffs are MyGo's editor: read-only editor.View for code and
// text, editor.DiffView for changes. Their look comes only from the
// editor's Theme, editor.DefaultTheme with the app theme's slots.

// codeFont is Kit's mono text: 13 DIPs.
var codeFont = ui.Font{Family: "monospace", Size: 13}

// codeLine is Kit's code line height, a multiple of the size: 1.6
// (content diff view).
const codeLine = 1.6

type codeThemeKey struct {
	p    *theme.Palette
	tint bool
}

var codeThemes sync.Map // codeThemeKey -> *editor.Theme

// CodeTheme is the editor's theme for the app theme: DefaultTheme with
// the text, gutter, selection and change colors from the palette, on no
// background. tint gives the background of code blocks in messages, the
// foreground at 3.5% over the background.
func CodeTheme(c *ui.Context, tint bool) *editor.Theme {
	p := theme.Of(c)
	key := codeThemeKey{p, tint}
	if t, ok := codeThemes.Load(key); ok {
		return t.(*editor.Theme)
	}
	t := editor.DefaultTheme(c.Theme())
	// Transparent: file tabs, sources and diffs show the surface they sit
	// on, with no block ending at the last line, and no current-line band
	// (read-only, as the reference's views).
	t.Background = p.Background.Alpha(0)
	t.ActiveLine = p.Background.Alpha(0)
	if tint {
		t.Background = p.Background.Mix(p.Foreground, 0.035)
	}
	t.Text = p.Foreground
	t.LineNumber = p.MutedForeground.Alpha(0.6)
	t.ActiveLineNumber = p.MutedForeground
	t.Selection = p.Selection
	t.InactiveSelection = p.Selection.Alpha(0.55)
	// Every view of this theme is read-only: the caret is hidden, as
	// MyGo's editor has no option to draw none (it keeps the keyboard's
	// selection and Copy). The color is the only switch.
	t.Cursor = p.Caret.Alpha(0)
	t.Added, t.Deleted = p.Success, p.Danger
	codeThemes.Store(key, t)
	return t
}

// codeView keeps an editor of a block of code and the text it shows.
type codeView struct {
	st   *editor.State
	text string
}

// CodeOptions are how Code shows its text.
type CodeOptions struct {
	// Language highlights the text; nil leaves it plain.
	Language *editor.Language
	// Numbers shows line numbers, as file tabs do.
	Numbers bool
	// Tint is the background of code in messages.
	Tint bool
}

// Code is a read-only editor of text, as high as its lines, inside box:
// it highlights by Language and keeps its editor while box stays.
// Trailing line breaks are left out, so no empty last line shows.
func Code(c *ui.Context, box ui.Element, text string, o CodeOptions) {
	text = strings.TrimRight(text, "\r\n")
	v := ui.Local(box, codeView{}, func() codeView {
		st := &editor.State{Buffer: editor.NewBuffer(text), ReadOnly: true, FitContent: true, NoScrollbars: true,
			NoActiveLine: true, NoFolding: true, NoIndentGuides: true, Font: codeFont, LineHeight: codeLine}
		st.Buffer.SetLanguage(o.Language)
		if !o.Numbers {
			st.LineNumbers = editor.LineNumbersOff
		}
		return codeView{st: st, text: text}
	})
	if v.text != text {
		v.text = text
		v.st.Buffer.Set(text)
	}
	v.st.Theme = CodeTheme(c, o.Tint)
	box.Children(func() { editor.View(c, v.st).FillWidth() })
}

// CodeBlock is the Markdown hook (Options.CodeBlock) for code in
// messages and documents: the language and Copy over a read-only editor
// highlighting by the fence's language.
func CodeBlock(c *ui.Context, b *markdown.CodeBlockView) {
	p := theme.Of(c)
	ui.Column(c).AlignSelf(ui.Stretch).Background(p.Background.Mix(p.Foreground, 0.035)).Border(1, p.Border).
		Radius(p.Radius).Clip().Children(func() {
		ui.Row(c).AlignItems(ui.Center).Padding(4, 6, 0, 12).Children(func() {
			ui.Text(c, b.Language).FontSize(12).TextColor(p.MutedForeground).Unselectable()
			ui.Spacer(c)
			b.CopyButton(c)
		})
		body := ui.Column(c).Padding(4, 0, 8, 4)
		Code(c, body, b.Code(), CodeOptions{Language: editor.LanguageNamed(b.Language), Tint: true})
	})
}

// diffView keeps a diff viewer and the texts it compares.
type diffView struct {
	d        *editor.DiffState
	old, new string
}

// DiffOptions are how Diff shows a change.
type DiffOptions struct {
	// Language highlights both sides.
	Language *editor.Language
	// Fit makes the viewer as high as its lines, inside a list or
	// scroll, rather than scrolling them itself.
	Fit bool
	// NoNumbers leaves the gutter without numbers, as transcript
	// previews.
	NoNumbers bool
	// Actions builds the actions of the change under the pointer, around
	// editor.DefaultHunkActions; nil shows none. Changed is called after
	// Revert or Keep changed a text.
	Actions func(c *ui.Context, bar ui.Element, a *editor.DiffHunkActions)
	Changed func()
}

// Diff is MyGo's diff viewer of old and new inside box, unified as
// Rust's (content diff view), read-only, keeping its state while box
// stays.
func Diff(c *ui.Context, box ui.Element, old, new string, o DiffOptions) {
	v := ui.Local(box, diffView{}, func() diffView {
		d := &editor.DiffState{Old: editor.NewBuffer(old), New: editor.NewBuffer(new), Layout: editor.DiffUnified,
			Indicators: editor.DiffClassic, FitContent: o.Fit, NoScrollbars: o.Fit, NoLineNumbers: o.NoNumbers,
			Font: codeFont, LineHeight: codeLine}
		d.Old.SetLanguage(o.Language)
		d.New.SetLanguage(o.Language)
		return diffView{d: d, old: old, new: new}
	})
	if v.old != old || v.new != new {
		v.old, v.new = old, new
		v.d.Old.Set(old)
		v.d.New.Set(new)
	}
	v.d.Theme = CodeTheme(c, false)
	v.d.HunkActions = o.Actions != nil
	v.d.RenderHunkActions = nil
	if o.Actions != nil {
		v.d.RenderHunkActions = func(c *ui.Context, bar ui.Element, a *editor.DiffHunkActions) {
			before := v.d.Old.Version() + v.d.New.Version()
			o.Actions(c, bar, a)
			if o.Changed != nil && v.d.Old.Version()+v.d.New.Version() != before {
				o.Changed()
			}
		}
	}
	box.Children(func() {
		e := editor.DiffView(c, v.d)
		if o.Fit {
			e.FillWidth()
		} else {
			e.Grow(1).MinHeight(0)
		}
	})
}
