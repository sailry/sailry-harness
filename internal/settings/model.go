package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/preview"
)

// The settings' view model: the boundary between the pages and their
// logic (docs/settings.md). Pages read and edit these fields; the logic
// half loads them, validates and saves them, and reports how a save went.
//
// Two kinds of settings, as in the reference:
//   - App preferences (preferences.rs Data) are the app's own and apply as
//     they change; they are plain fields.
//   - Node settings (power, telemetry, sandbox, the terminal host, hooks,
//     the Python index, local models, backup) are a host's, saved by a
//     revisioned request: Stored keeps the saved value, the draft the page
//     edits and the revision the draft is based on.

// Stored is a Node setting: Saved at Revision, the Draft the page edits,
// and Pending while a save is under way, which locks the page's controls.
type Stored[T any] struct {
	Saved, Draft T
	Revision     uint64
	Pending      bool
}

// Stored creates a setting saved as v.
func stored[T any](v T) Stored[T] { return Stored[T]{Saved: v, Draft: v, Revision: 1} }

// Commit saves the draft. The preview has no Node: the draft becomes the
// saved value in memory and the save is Done. The logic half sends the
// request with Revision and returns its outcome: Failed keeps the draft,
// Conflict keeps it until the saved value is read again, Uncertain keeps
// it while the result is unknown.
func (s *Stored[T]) Commit() kit.Outcome {
	s.Saved = s.Draft
	s.Revision++
	return kit.Done
}

// Messages are the toasts of a save's outcomes, by i18n key; "" shows
// none, as the reference's switches show no success toast.
type Messages struct{ Done, Failed, Conflict, Uncertain string }

// report shows a save's outcome as a toast, the only place outcomes show.
func report(c *ui.Context, o kit.Outcome, m Messages) {
	key := map[kit.Outcome]string{kit.Done: m.Done, kit.Failed: m.Failed, kit.Conflict: m.Conflict, kit.Uncertain: m.Uncertain}[o]
	if key != "" {
		kit.Toast(c, o, i18n.T(c, key))
	}
}

// notice is the toast of an action the preview does not carry out: one
// that would read the disk, use the network, pair or run a model.
func notice(c *ui.Context) { c.Toast(i18n.T(c, "preview_notice")) }

// Telemetry is what diagnostics send (telemetry.rs).
type Telemetry struct{ Errors, Metrics bool }

// LocalModels is a kind of local model's page (models/view.rs): enabled,
// the model chosen among the installed ones, the Installed or Available
// tab, and the models.
type LocalModels struct {
	Enabled Stored[bool]
	Model   int // index of the installed models
	Tab     int // 0 installed, 1 available
	Query   string
	Models  []preview.LocalModel
	Catalog []preview.LocalModel
}

// Installed are the models a host has.
func (m *LocalModels) Installed() []preview.LocalModel {
	var out []preview.LocalModel
	for _, x := range m.Models {
		if x.Status == preview.ModelInstalled {
			out = append(out, x)
		}
	}
	return out
}

// Model is the settings pages' view model.
type Model struct {
	// General (preferences.rs, power.rs, telemetry.rs).
	Language       int // of Languages
	SidebarMetrics bool
	Notify         [4]bool // system, approval, completed, failed
	Toast          int     // of ToastSeconds
	Power          Stored[bool]
	Telemetry      Stored[Telemetry]

	// Conversation (conversation.rs, conversation/sandbox.rs).
	Sandbox        Stored[bool]
	SessionOrder   int     // 0 created, 1 updated
	MessageDisplay int     // 0 detailed, 1 compact
	TextSize       float64 // 12..18 by 2

	// Shortcuts (settings/shortcuts.rs, shortcuts.rs).
	Keys Keys

	// Dictation (dictation.rs).
	MicrophoneGranted bool
	Microphone        int // 0 default, then Microphones
	Microphones       []string
	SpeechLanguage    int // 0 auto, 1 Chinese, 2 English
	SpeechModel       int // of Speech
	Speech            []preview.LocalModel
	info              *modelInfo // the model whose details show
}

// Languages are the language choices (locale.rs Language::ALL).
var Languages = []string{
	"settings_language_system", "settings_language_english", "settings_language_chinese",
	"settings_language_traditional_chinese", "settings_language_japanese", "settings_language_korean",
	"settings_language_french", "settings_language_german", "settings_language_spanish",
	"settings_language_portuguese_brazil", "settings_language_russian",
}

// ToastSeconds are the toast durations offered (preferences.rs duration).
var ToastSeconds = []int{3, 5, 8, 10}

// NewModel builds the view model over the settings' dataset.
func NewModel(d *preview.Settings) *Model {
	m := &Model{
		Notify: [4]bool{true, true, true, true}, Toast: 1,
		Power:     stored(d.PreventSleep),
		Telemetry: stored(Telemetry{Errors: d.Errors, Metrics: d.Metrics}),
		Sandbox:   stored(d.Sandbox), TextSize: 14,
		Keys:        NewKeys(),
		Microphones: d.Microphones, Speech: d.Speech,
	}
	return m
}
