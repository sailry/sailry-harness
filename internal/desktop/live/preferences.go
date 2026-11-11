package live

import (
	"encoding/json"
	"maps"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func (r *Runtime) applyPreferences() {
	d := r.Preferences.Data
	a, m := r.App, r.App.Settings.Model
	m.Notify, m.SidebarMetrics = d.Notifications, d.SidebarMetrics
	m.Language, m.MessageDisplay, m.SessionOrder, m.Channel = 0, 0, int(sidebar.SortUpdated), 0
	m.TextSize = 14
	m.SpeechLanguage = max(0, slices.Index(preferences.DictationLanguages, d.DictationLanguage()))
	if d.Language != nil {
		m.Language = max(0, slices.Index(preferences.Languages, *d.Language))
	}
	for i, seconds := range settings.ToastSeconds {
		if uint64(seconds) == d.ToastSeconds {
			m.Toast = i
		}
	}
	if d.MessageDisplay != nil && *d.MessageDisplay == "compact" {
		m.MessageDisplay = 1
	}
	if d.SessionOrder != nil {
		m.SessionOrder = int(sidebar.SortOf(*d.SessionOrder))
	}
	if d.MessageTextSize != nil {
		m.TextSize = float64(*d.MessageTextSize)
	}
	if d.UpdateChannel != nil && *d.UpdateChannel == "experience" {
		m.Channel = 1
	}
	m.FontSize, m.Font, m.PasteProtection = float64(d.Terminal.FontSize), d.Terminal.FontFamily, d.Terminal.PasteProtection
	a.Theme = d.Appearance.Package
	if a.Theme == "builtin" {
		a.Theme = ""
	}
	switch d.Appearance.Mode {
	case "light":
		a.Mode = theme.Light
	case "dark":
		a.Mode = theme.Dark
	default:
		a.Mode = theme.System
	}
	if d.ThemeEffects != nil {
		a.Effects = *d.ThemeEffects
	} else {
		a.Effects = true
	}
	a.Surfaces = d.Materials()
	r.lastMaterials = [3]float32{a.Surfaces.Main, a.Surfaces.Sidebar, a.Surfaces.Blur}
	m.Keys.SetOverrides(nil)
	if d.Shortcuts != nil {
		m.Keys.SetOverrides(*d.Shortcuts)
	}
	order, pins := []string(nil), map[string]bool(nil)
	if d.FeatureOrder != nil {
		order = *d.FeatureOrder
	}
	if d.FeaturePins != nil {
		pins = *d.FeaturePins
	}
	a.SetFeaturePreferences(order, pins)
	if d.WelcomeSeen != nil {
		a.SetWelcome(*d.WelcomeSeen)
	}
	order, pins = a.FeaturePreferences()
	r.lastFeatures = encoded([]any{order, pins})
	r.lastKeys = encoded(m.Keys.Overrides())
	r.lastPreferences = encoded(d)
}

func encoded(v any) string { b, _ := json.Marshal(v); return string(b) }
func setChoice[T comparable](field **T, value, fallback T) {
	if *field != nil && **field == value || *field == nil && value == fallback {
		return
	}
	*field = &value
}

// savePreferences captures edits on the UI thread and writes one immutable JSON
// snapshot off-thread. Failed bytes are not silently retried every frame.
func (r *Runtime) savePreferences() {
	a, m := r.App, r.App.Settings.Model
	d := &r.Preferences.Data
	d.Notifications, d.SidebarMetrics = m.Notify, m.SidebarMetrics
	if m.Toast >= 0 && m.Toast < len(settings.ToastSeconds) {
		d.ToastSeconds = uint64(settings.ToastSeconds[m.Toast])
	}
	if m.Language >= 0 && m.Language < len(preferences.Languages) {
		setChoice(&d.Language, preferences.Languages[m.Language], "system")
	}
	setChoice(&d.MessageDisplay, []string{"detailed", "compact"}[min(max(m.MessageDisplay, 0), 1)], "detailed")
	// The sidebar orders its rows by the choice; a profile that never chose
	// keeps none, which is updated.
	setChoice(&d.SessionOrder, sidebar.Sort(m.SessionOrder).Preference(), sidebar.SortUpdated.Preference())
	setChoice(&d.MessageTextSize, uint8(m.TextSize), uint8(14))
	if language := preferences.DictationLanguages[min(max(m.SpeechLanguage, 0), 2)]; language != d.DictationLanguage() {
		if d.Dictation == nil {
			d.Dictation = &preferences.Dictation{}
		}
		d.Dictation.Language = language
	}
	setChoice(&d.UpdateChannel, []string{"stable", "experience"}[min(max(m.Channel, 0), 1)], "stable")
	d.Terminal.FontSize, d.Terminal.FontFamily, d.Terminal.PasteProtection = uint8(m.FontSize), m.Font, m.PasteProtection
	packageID := a.Theme
	if packageID == "" {
		packageID = "builtin"
	}
	d.Appearance = preferences.Selection{Package: packageID, Mode: []string{"system", "light", "dark"}[min(max(int(a.Mode), 0), 2)]}
	setChoice(&d.ThemeEffects, a.Effects, true)
	setChoice(&d.WelcomeSeen, a.Welcome.Seen, true)
	now := [3]float32{a.Surfaces.Main, a.Surfaces.Sidebar, a.Surfaces.Blur}
	if now != r.lastMaterials {
		if d.Surfaces == nil {
			d.Surfaces = &preferences.Surfaces{Main: 83.5}
		}
		if now[0] != r.lastMaterials[0] {
			d.Surfaces.Main = now[0] * 100
		}
		if now[1] != r.lastMaterials[1] {
			value := now[1] * 100
			d.Surfaces.Sidebar = &value
		}
		if now[2] != r.lastMaterials[2] {
			value := now[2] * 100
			d.Surfaces.Blur = &value
		}
		r.lastMaterials = now
	}
	order, pins := a.FeaturePreferences()
	if current := encoded([]any{order, pins}); current != r.lastFeatures {
		d.FeatureOrder = &order
		d.FeaturePins = &pins
		r.lastFeatures = current
	}
	if keys := m.Keys.Overrides(); encoded(keys) != r.lastKeys {
		all := map[string]string{}
		if d.Shortcuts != nil {
			all = maps.Clone(*d.Shortcuts)
		}
		for _, s := range settings.Registry {
			delete(all, s.ID)
			if value, ok := keys[s.ID]; ok {
				all[s.ID] = value
			}
		}
		d.Shortcuts = &all
		r.lastKeys = encoded(keys)
	}
	current := encoded(d)
	if current == r.lastPreferences {
		return
	}
	bytes, err := json.MarshalIndent(d, "", "  ")
	if err != nil {
		return
	}
	r.lastPreferences = current
	if !r.Preferences.Readable {
		return
	}
	if err = d.Validate(); err != nil {
		r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "preferences_save_failed")) })
		return
	}
	version := r.Preferences.Changed()
	r.prefsRevision = version
	if r.preferencesChanged != nil {
		r.preferencesChanged()
	}
	r.async(func() {
		err := r.Preferences.WriteSnapshot(version, bytes)
		r.enqueue(func(c *ui.Context) {
			if err != nil {
				kit.Toast(c, kit.Failed, i18n.T(c, "preferences_save_failed"))
			}
		})
	})
}
