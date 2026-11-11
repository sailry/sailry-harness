package panel

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

type prompt struct {
	open, pending  bool
	locked         bool
	title          string
	target, action string
	baseAction     string
	message        string
	labels, values []string
	submit         func([]string, func(bool))
	toggles        []PromptToggle
	identity       *bool
}
type PromptToggle struct {
	Label string
	Value *bool
}

func (pr *Project) LockPrompt(locked bool)     { pr.prompt.locked = locked }
func (pr *Project) SetPromptAction(key string) { pr.prompt.action, pr.prompt.baseAction = key, key }
func (pr *Project) SetPromptUncertain(uncertain bool) {
	pr.prompt.locked = uncertain
	pr.prompt.action = pr.prompt.baseAction
	if uncertain {
		pr.prompt.action = "files_check_result"
	}
}

func (pr *Project) Confirm(title, target string, submit func(func(bool)), consequence ...string) {
	message := ""
	if len(consequence) > 0 {
		message = consequence[0]
	}
	pr.prompt = prompt{open: true, identity: new(bool), title: title, target: target, action: title, baseAction: title, message: message, submit: func(_ []string, done func(bool)) { submit(done) }}
}

// Prompt opens a required operation form. A failed or uncertain save keeps the
// form values; the controller reports its single outcome through a toast.
func (pr *Project) Prompt(title string, labels, values []string, submit func([]string, func(bool)), toggles ...PromptToggle) {
	pr.prompt = prompt{open: true, identity: new(bool), title: title, labels: append([]string(nil), labels...), values: append([]string(nil), values...), submit: submit, toggles: toggles}
}
func (pr *Project) submitPrompt() {
	form := &pr.prompt
	if !form.open || form.pending || form.submit == nil {
		return
	}
	form.pending = true
	identity := form.identity
	form.submit(append([]string(nil), form.values...), func(success bool) {
		if form.identity != identity {
			return
		}
		form.pending = false
		if success {
			form.open = false
		}
	})
}
func (pr *Project) promptDialog(c *ui.Context) {
	pr.editorCloseDialog(c)
	form := &pr.prompt
	if !form.open {
		return
	}
	palette := theme.Of(c)
	title := i18n.T(c, form.title, "branch", form.target)
	kit.Dialog(c.Key("workspace-form"), &form.open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600).TextColor(palette.PopoverForeground)
		ui.Column(c.Key("body")).Gap(16).Children(func() {
			if form.message != "" {
				ui.Text(c, i18n.T(c, form.message)).FontSize(14).TextColor(palette.PopoverForeground)
			}
			if form.target != "" {
				ui.Text(c, form.target).FontSize(14).TextColor(palette.PopoverForeground)
			}
			for index, key := range form.labels {
				label := i18n.T(c, key)
				kit.Field(c, index, label, "", func() {
					ui.TextInput(c.Key("input"), &form.values[index]).Label(label).FillWidth().Disabled(form.pending || form.locked).OnSubmit(pr.submitPrompt)
				})
			}
			for index, toggle := range form.toggles {
				if toggle.Value != nil {
					ui.Checkbox(c.Key("toggle").Key(index), toggle.Value, i18n.T(c, toggle.Label)).Disabled(form.pending || form.locked)
				}
			}
		})
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).Disabled(form.pending).OnClick(func() { form.open = false })
			label := i18n.T(c, "settings_save")
			if form.action != "" {
				label = i18n.T(c, form.action)
			}
			kit.PrimaryBusy(c.Key("save"), label, false, form.pending).OnClick(pr.submitPrompt)
		})
	}).Width(480).Label(title)
}
