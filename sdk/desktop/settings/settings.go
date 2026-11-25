// Package settings exposes native settings contributions and their shared drafts.
package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

type NativePage = settings.NativePage
type State = settings.State
type Model = settings.Model
type Section = settings.Section
type TerminalHost = settings.TerminalHost
type TerminalEnvEditor = settings.TerminalEnvEditor
type EnvVar = preview.EnvVar
type Shell = preview.Shell
type DraftField = settings.DraftField
type ProviderHost = settings.ProviderHost
type Look = settings.Look

const Terminal = settings.Terminal

func New() *State     { return settings.New() }
func NewLive() *State { return settings.NewLive() }

func Page(c *ui.Context, state *State, look Look) {
	settings.Page(c, state, nil, look)
}

func DraftDialog(c *ui.Context, key any, open *bool, title string, fields []DraftField, save func() string) {
	settings.DraftDialog(c, key, open, title, fields, save)
}

// Pick fills the available row and reports whether its indexed selection changed.
func Pick(c *ui.Context, key any, title string, chosen *int, options []string, disabled bool) bool {
	return settings.Pick(c, key, title, chosen, options, disabled).Fill().Changed()
}

func ReportTerminal(c *ui.Context, outcome kit.Outcome) {
	settings.Report(c, outcome, settings.TerminalMessages)
}
