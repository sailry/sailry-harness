package composer

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Voice input: a round ghost button, the microphone while idle, a spinner
// while the speech model prepares or the recording transcribes, and while
// recording the last four levels of the microphone as bars, the button
// selected. Idle it starts, recording it finishes, otherwise it cancels.

// DictationCommand is what the button asks of the backend.
type DictationCommand int

const (
	StartDictation DictationCommand = iota
	FinishDictation
	CancelDictation
)

// DictationCaret is where a transcript goes: the draft's caret as the last
// frame saw it, in runes.
func (s *State) DictationCaret() int {
	return min(max(s.pluginEdit.selection[0], s.pluginEdit.selection[1], 0), len([]rune(s.Draft)))
}

// ReplaceDictation puts a transcript in place of the draft's runes from
// start to end and the caret after it. It does nothing while the input
// composes or when the range is not the draft's.
func (s *State) ReplaceDictation(start, end int, text string) bool {
	runes := []rune(s.Draft)
	if s.pluginEdit.composing || start < 0 || end < start || end > len(runes) {
		return false
	}
	s.Draft = string(runes[:start]) + text + string(runes[end:])
	caret := start + len([]rune(text))
	s.pluginEdit.selection = [2]int{caret, caret}
	s.pluginEdit.pending = &[2]int{caret, caret}
	s.focus = true
	return true
}

func (s *State) dictation(c *ui.Context, ctx *Context, actions Actions) {
	d := &s.Dictation
	label := [...]string{"dictation_start", "dictation_preparing", "dictation_finish", "dictation_transcribing"}[d.Phase]
	b := kit.GhostLook(c, ui.ButtonBase(c.Key("dictation")), d.Phase == Recording).Label(i18n.T(c, label)).
		Tooltip(i18n.T(c, label)).Size(32, 32).Radius(16).Disabled(ctx.Readonly).AccessSelected(d.Phase == Recording)
	b.Children(func() {
		switch d.Phase {
		case Idle:
			ui.Icon(c, icons.Microphone).FontSize(16)
		case Recording:
			waveform(c, d.Levels)
		default:
			ui.Spinner(c).FontSize(16)
		}
	}).OnClick(func() {
		switch d.Phase {
		case Idle:
			actions.Dictate(d, StartDictation)
		case Recording:
			actions.Dictate(d, FinishDictation)
		default:
			actions.Dictate(d, CancelDictation)
		}
	})
}

// waveform is four bars 2 wide, 1.5 apart, 3 to 12 high by the last levels,
// oldest first. Custom drawing: a meter of live levels, which MyGo has no
// widget for.
func waveform(c *ui.Context, levels []float32) {
	p := theme.Of(c)
	var bars [4]float32
	n := min(len(levels), 4)
	copy(bars[4-n:], levels[len(levels)-n:])
	ui.Box(c.Key("waveform")).Size(16, 16).PassThrough().Draw(func(pt *ui.Painter, r ui.Rect) {
		x := r.X + (r.W-(4*2+3*1.5))/2
		for _, l := range bars {
			h := 3 + min(max(l, 0), 1)*9
			pt.Fill(ui.Rect{X: x, Y: r.Y + (r.H-h)/2, W: 2, H: h}, p.Foreground, 1)
			x += 3.5
		}
	})
}
