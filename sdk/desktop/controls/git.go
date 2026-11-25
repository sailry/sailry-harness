package controls

import (
	"log/slog"

	"github.com/ELY-Open-Source/CrychicUI/code"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/kit/crychic"
)

type (
	CodeOptions    = kit.CodeOptions
	DiffOptions    = kit.DiffOptions
	CommandItem    = kit.CommandItem
	CommandProps   = kit.CommandProps
	Menu           = kit.Menu
	MenuOptions    = kit.MenuOptions
	SegmentedProps = kit.SegmentedProps
	SplitProps     = kit.SplitProps
)

const (
	SizeSmall = kit.SizeSmall
)

func Code(c *ui.Context, box ui.Element, text string, opts CodeOptions) { kit.Code(c, box, text, opts) }
func Diff(c *ui.Context, box ui.Element, old, next string, opts DiffOptions) {
	kit.Diff(c, box, old, next, opts)
}
func CommitGraph(c *ui.Context, label string, commits []code.GraphCommit) (ui.Element, bool) {
	return crychic.CommitGraph(c, label, commits)
}
func CommitGraphPad(c *ui.Context) float32              { return crychic.CommitGraphPad(c) }
func CommitGraphLanes(c *ui.Context, lanes int) float32 { return crychic.CommitGraphLanes(c, lanes) }
func CommitGraphRows(c *ui.Context) (float32, float32)  { return crychic.CommitGraphRows(c) }
func Command(c *ui.Context, key any, open *bool, query *string, index *int, items []CommandItem, props CommandProps, pick func(int)) {
	kit.Command(c, key, open, query, index, items, props, pick)
}
func AlertDialog(c *ui.Context, open *bool, title, message string, buttons ...string) int {
	return kit.AlertDialog(c, open, title, message, buttons...)
}
func BusyAction(c *ui.Context, label string) ui.Element { return kit.BusyAction(c, label) }
func IconLook(c *ui.Context, b ui.Element, icon *ui.SVG, label, hint string, size float32, selected bool) {
	kit.IconLook(c, b, icon, label, hint, size, selected)
}
func Loader(c *ui.Context, size float32) ui.Element { return kit.Loader(c, size) }
func MenuButton(c *ui.Context, id string, look func(ui.Element), build func(*Menu), options ...MenuOptions) ui.Element {
	return kit.MenuButton(c, id, look, build, options...)
}
func Primary(c *ui.Context, label string, disabled bool) ui.Element {
	return kit.Primary(c, label, disabled)
}
func Secondary(c *ui.Context, label string) ui.Element { return kit.Secondary(c, label) }
func Segmented(c *ui.Context, key any, selected *int, options []string, props SegmentedProps) ui.Element {
	return kit.Segmented(c, key, selected, options, props)
}
func SplitButton(c *ui.Context, id, label, more string, props SplitProps, build func(*Menu)) ui.Element {
	return kit.SplitButton(c, id, label, more, props, build)
}
func Tooltip(c *ui.Context, anchor ui.Element, text string) { kit.Tooltip(c, anchor, text) }
func Demo(c *ui.Context) bool                               { return kit.Demo(c) }
func PreviewNotice(c *ui.Context)                           { kit.PreviewNotice(c) }

const (
	Failed    = kit.Failed
	Conflict  = kit.Conflict
	Uncertain = kit.Uncertain
)

func RecordFailure(operation string, outcome Outcome, err error, attrs ...slog.Attr) {
	kit.RecordFailure("desktop", operation, outcome, err, attrs...)
}

const (
	Positive = kit.Positive
	Negative = kit.Negative
)
