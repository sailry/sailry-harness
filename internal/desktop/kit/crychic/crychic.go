// Package crychic holds Sailry's policy for CrychicUI
// (github.com/ELY-Open-Source/CrychicUI), the application's second UI
// library after MyGo's own controls. It is not a second Kit: a component that
// is safe with the data it is given is called from its own package, and only
// the ones that panic on data Sailry does not author, or that would cover a
// Kit floating surface, have an adapter here. The verified components, the
// copy every caller supplies and the caveats are in docs/crychic.md.
package crychic

import (
	"math"
	"net/url"
	"strconv"
	"strings"

	"github.com/ELY-Open-Source/CrychicUI/agent"
	"github.com/ELY-Open-Source/CrychicUI/chat"
	"github.com/ELY-Open-Source/CrychicUI/code"
	"github.com/ELY-Open-Source/CrychicUI/desktop"
	"github.com/ELY-Open-Source/CrychicUI/devtools"
	"github.com/ELY-Open-Source/CrychicUI/media"
	"github.com/ELY-Open-Source/CrychicUI/navigation"
	"github.com/ELY-Open-Source/CrychicUI/overlay"
	"github.com/ELY-Open-Source/CrychicUI/sound"
	"github.com/egoist/mygo/ui"
)

// Setup applies Sailry's policy to the library; Desktop calls it once before
// its first window. The library plays a synthesized cue on every interaction
// unless told otherwise, and a muted cue returns before any sound is rendered
// or the audio output is started.
func Setup() { sound.SetEnabled(false) }

// Unit keeps a share inside 0 to 1, a missing one (NaN) at 0: the components
// panic on a level, a progress or a played share outside it.
func Unit(v float64) float64 {
	if math.IsNaN(v) {
		return 0
	}
	return min(max(v, 0), 1)
}

// ImageGenerating is agent.ImageGenerating for a prompt and a progress that
// come from a model run. It builds nothing for an empty prompt.
func ImageGenerating(c *ui.Context, prompt string, progress float64) (ui.Element, bool) {
	if prompt == "" {
		return ui.Element{}, false
	}
	return agent.ImageGenerating(c, prompt, Unit(progress)), true
}

// source completes a cited page: a missing host is read from the address
// and a missing title is the host. A source without an address, or still
// without a host, cannot be shown.
func source(n int, s agent.Source) (agent.Source, bool) {
	if s.Host == "" {
		if u, err := url.Parse(s.URL); err == nil {
			s.Host = u.Hostname()
		}
	}
	if s.Title == "" {
		s.Title = s.Host
	}
	return s, n >= 1 && s.URL != "" && s.Host != ""
}

// SourceCard is agent.SourceCard for a source a model or a search returned.
func SourceCard(c *ui.Context, n int, src agent.Source) (ui.Element, bool) {
	src, ok := source(n, src)
	if !ok {
		return ui.Element{}, false
	}
	return agent.SourceCard(c, n, src), true
}

// Citation is agent.Citation for a source a model or a search returned.
func Citation(c *ui.Context, n int, src agent.Source) (ui.Element, bool) {
	src, ok := source(n, src)
	if !ok {
		return ui.Element{}, false
	}
	return agent.Citation(c, n, src), true
}

// RunTimeline is agent.RunTimeline for the steps of a run in flight. Unnamed
// steps are left out, a negative duration counts as none and an unknown
// state as pending. It builds nothing until some step has taken time.
func RunTimeline(c *ui.Context, label string, steps []agent.RunStep, o agent.RunTimelineOptions) (ui.Element, bool) {
	var kept []agent.RunStep
	var total int64
	for _, s := range steps {
		if s.Name == "" {
			continue
		}
		s.Duration = max(s.Duration, 0)
		if s.State > agent.StepFailed {
			s.State = agent.StepPending
		}
		total += int64(s.Duration)
		kept = append(kept, s)
	}
	if label == "" || total <= 0 {
		return ui.Element{}, false
	}
	return agent.RunTimeline(c, label, kept, o), true
}

// TaskProgress is agent.TaskProgress for steps a model named. An unnamed
// step shows as an ellipsis, so the count stays the model's, and current
// stays within the steps.
func TaskProgress(c *ui.Context, label string, steps []string, current int, o agent.TaskProgressOptions) (ui.Element, bool) {
	if label == "" || len(steps) == 0 {
		return ui.Element{}, false
	}
	named := make([]string, len(steps))
	for i, s := range steps {
		if named[i] = s; s == "" {
			named[i] = "…"
		}
	}
	return agent.TaskProgress(c, label, named, min(max(current, 0), len(named)), o), true
}

// LinkPreview is chat.LinkPreview for a page's own metadata: an untitled
// page shows its host as the title.
func LinkPreview(c *ui.Context, title, host string, image *ui.Bitmap) (ui.Element, bool) {
	if host == "" {
		return ui.Element{}, false
	}
	if title == "" {
		title = host
	}
	return chat.LinkPreview(c, title, host, image), true
}

// Lightbox is overlay.Lightbox for decoded attachments. It builds nothing,
// and closes, when there is no image or one failed to decode; the shown
// index stays within the images and captions that do not match are dropped.
// The component is blocked (docs/crychic.md): it draws its own scrim and
// Kit's image viewer does more. The adapter is kept for when that changes.
func Lightbox(c *ui.Context, label string, images []*ui.Bitmap, captions []string, at *int, open *bool, o overlay.LightboxOptions) (ui.Element, bool) {
	ok := label != "" && len(images) > 0
	for _, img := range images {
		ok = ok && img != nil
	}
	if !ok {
		*open = false
		return ui.Element{}, false
	}
	if captions != nil && len(captions) != len(images) {
		captions = nil
	}
	*at = min(max(*at, 0), len(images)-1)
	return overlay.Lightbox(c, label, images, captions, at, open, o), true
}

// ShortcutSheet is overlay.ShortcutSheet for the user's own key bindings:
// an action without keys is left out, and so is a group left empty. The
// component is blocked (docs/crychic.md): its panel is opaque, not Kit's
// frost. The adapter is kept for when the library takes a surface.
func ShortcutSheet(c *ui.Context, title string, groups []overlay.ShortcutGroup, open *bool, o overlay.ShortcutSheetOptions) (ui.Element, bool) {
	var kept []overlay.ShortcutGroup
	for _, g := range groups {
		var bound []overlay.Shortcut
		for _, s := range g.Shortcuts {
			if s.Label != "" && len(s.Keys) > 0 {
				bound = append(bound, s)
			}
		}
		if g.Title != "" && len(bound) > 0 {
			kept = append(kept, overlay.ShortcutGroup{Title: g.Title, Shortcuts: bound})
		}
	}
	if title == "" || len(kept) == 0 {
		*open = false
		return ui.Element{}, false
	}
	return overlay.ShortcutSheet(c, title, kept, open, o), true
}

// UpdatePrompt is desktop.UpdatePrompt as the content of a Kit floating
// surface (kit.Dialog, kit.Popover). On its own the prompt is an opaque
// card, which would cover the frost of the surface it sits in: here its
// fill and its edge are cleared, so that surface stays the only one. The
// Later button's outline goes with the edge, and the card's faint shadow,
// which is in the text color, stays. On a page, where nothing is under it,
// call the library's own.
func UpdatePrompt(c *ui.Context, app, version string, notes []string, o desktop.UpdatePromptOptions) (root, install, later ui.Element) {
	bare := *c.Theme()
	bare.Background, bare.Border = ui.Transparent, ui.Transparent
	ui.Themed(c, &bare).FillWidth().Children(func() {
		root, install, later = desktop.UpdatePrompt(c, app, version, notes, o)
	})
	return root, install, later
}

// ZoomControl is navigation.ZoomControl for a stored zoom: one outside the
// range moves to its nearest end, and a missing one to the fit. The range in
// o is the caller's own and is not repaired.
func ZoomControl(c *ui.Context, label string, zoom *float64, o navigation.ZoomControlOptions) ui.Element {
	lo, hi, fit := o.Min, o.Max, o.FitTo
	if lo == 0 {
		lo = 0.25
	}
	if hi == 0 {
		hi = 4
	}
	if fit == 0 {
		fit = 1
	}
	if math.IsNaN(*zoom) {
		*zoom = fit
	}
	*zoom = min(max(*zoom, lo), hi)
	return navigation.ZoomControl(c, label, zoom, o)
}

// Waveform is media.Waveform for measured levels, each kept inside 0 to 1.
func Waveform(c *ui.Context, label string, levels []float64, played float64, o media.WaveformOptions) (ui.Element, bool) {
	if label == "" || len(levels) == 0 {
		return ui.Element{}, false
	}
	kept := make([]float64, len(levels))
	for i, v := range levels {
		kept[i] = Unit(v)
	}
	return media.Waveform(c, label, kept, Unit(played), o), true
}

// LogView is devtools.LogView for records as they were written. A record
// without a message is left out, one without a time shows a dash, an unknown
// level reads as the nearest known one, and a missing or repeated ID is
// replaced by the record's position.
func LogView(c *ui.Context, lines []devtools.LogLine, query *string, follow *bool, o devtools.LogViewOptions) ui.Element {
	kept := make([]devtools.LogLine, 0, len(lines))
	seen := make(map[string]bool, len(lines))
	for i, l := range lines {
		if l.Message == "" {
			continue
		}
		if l.Time == "" {
			l.Time = "–"
		}
		l.Level = min(max(l.Level, devtools.LevelDebug), devtools.LevelError)
		if strings.TrimSpace(l.ID) == "" || seen[l.ID] {
			// An authored ID may already read as a position.
			for l.ID = "#" + strconv.Itoa(i); seen[l.ID]; l.ID += "'" {
			}
		}
		seen[l.ID] = true
		kept = append(kept, l)
	}
	return devtools.LogView(c, kept, query, follow, o)
}

// CommitGraph is code.CommitGraph for a repository's history. A commit
// without a message shows its hash and lanes fold onto the eight drawn. It
// builds nothing for a history the graph cannot draw: none, an unnamed or
// repeated commit, or a parent listed before its child.
func CommitGraph(c *ui.Context, label string, commits []code.GraphCommit) (ui.Element, bool) {
	if label == "" || len(commits) == 0 {
		return ui.Element{}, false
	}
	kept := make([]code.GraphCommit, len(commits))
	row := make(map[string]int, len(commits))
	for i, k := range commits {
		if k.Hash == "" || row[k.Hash] > 0 {
			return ui.Element{}, false
		}
		row[k.Hash] = i + 1
		if k.Message == "" {
			k.Message = k.Hash
		}
		k.Lane = max(k.Lane, 0) % 8
		kept[i] = k
	}
	for i, k := range kept {
		for _, p := range k.Parents {
			if j := row[p]; j > 0 && j-1 <= i {
				return ui.Element{}, false
			}
		}
	}
	return code.CommitGraph(c, label, kept), true
}

// BranchPicker is code.BranchPicker for a repository's branches. Counts
// below zero read as zero and the current index stays within the list. It
// builds nothing for no branch, or for an unnamed or repeated one.
func BranchPicker(c *ui.Context, label string, branches []code.Branch, current *int, query *string, o code.BranchPickerOptions) (root, field ui.Element, ok bool) {
	if label == "" || len(branches) == 0 {
		return ui.Element{}, ui.Element{}, false
	}
	kept := make([]code.Branch, len(branches))
	seen := make(map[string]bool, len(branches))
	for i, b := range branches {
		if b.Name == "" || seen[b.Name] {
			return ui.Element{}, ui.Element{}, false
		}
		seen[b.Name] = true
		b.Ahead, b.Behind = max(b.Ahead, 0), max(b.Behind, 0)
		kept[i] = b
	}
	*current = min(max(*current, 0), len(kept)-1)
	root, field = code.BranchPicker(c, label, kept, current, query, o)
	return root, field, true
}

// Blame is code.Blame for a file's history: a line without an author shows
// a dash. It builds nothing for no lines or for a line without a time.
func Blame(c *ui.Context, label string, lines []code.BlameLine, o code.BlameOptions) (ui.Element, bool) {
	if label == "" || len(lines) == 0 {
		return ui.Element{}, false
	}
	kept := make([]code.BlameLine, len(lines))
	for i, l := range lines {
		if l.At.IsZero() {
			return ui.Element{}, false
		}
		if l.Author == "" {
			l.Author = "–"
		}
		kept[i] = l
	}
	return code.Blame(c, label, kept, o), true
}
