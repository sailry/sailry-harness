package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

func (r *Runtime) sshCredentialFields(c *ui.Context, draft *settings.ConnectionDraft) {
	method := max(int(draft.Method)-1, 0)
	labels := []string{i18n.T(c, "ssh_password"), i18n.T(c, "ssh_private_key"), i18n.T(c, "ssh_key_file")}
	kit.Segmented(c, "authentication", &method, labels, kit.SegmentedProps{Block: true}).Label(i18n.T(c, "connection"))
	draft.Method = settings.Method(method + 1)
	field := func(key string, value *string, secret bool) {
		label := i18n.T(c, key)
		kit.Field(c, key, label, "", func() {
			input := ui.TextInput(c.Key("input"), value).Label(label).FillWidth()
			if secret {
				input.Password()
			}
		})
	}
	field("settings_name", &draft.Name, false)
	ui.Row(c.Key("address")).Gap(12).Children(func() {
		ui.Column(c.Key("host")).Grow(1).MinWidth(0).Children(func() { field("ssh_host", &draft.Host, false) })
		ui.Column(c.Key("port")).Width(96).Shrink(0).Children(func() { field("ssh_port", &draft.Port, false) })
	})
	field("ssh_username", &draft.Username, false)
	switch draft.Method {
	case settings.ByPassword:
		field("ssh_password", &draft.Password, true)
	case settings.ByPrivateKey:
		label := i18n.T(c, "ssh_private_key")
		kit.Field(c, "key", label, "", func() {
			ui.TextArea(c.Key("input"), &draft.Key).Label(label).Placeholder(i18n.T(c, "form_private_key_hint")).FillWidth().Height(96).Font("monospace")
		})
	case settings.ByKeyFile:
		label := i18n.T(c, "ssh_key_path")
		kit.Field(c, "path", label, "", func() {
			ui.Row(c).Gap(6).Children(func() {
				ui.TextInput(c.Key("input"), &draft.Key).Label(label).Grow(1)
				kit.IconButton(c.Key("browse"), icons.FolderOpen, i18n.T(c, "ssh_key_file"), 32, false).OnClick(func() {
					if r.pickKeyFile == nil {
						kit.Toast(c, kit.Failed, i18n.T(c, "unavailable"))
						return
					}
					expected := *draft
					r.pickKeyFile(func(path string) {
						r.enqueue(func(*ui.Context) {
							if path != "" && *draft == expected {
								draft.Key = path
							}
						})
					})
				})
			})
		})
	}
	if draft.Method != settings.ByPassword {
		field("ssh_passphrase", &draft.Passphrase, true)
	}
}
