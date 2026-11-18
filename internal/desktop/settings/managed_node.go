package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// ManagesNode reports whether this page configures a selected execution Node.
func (section Section) ManagesNode() bool {
	switch section {
	case Teams, Connectors, Media, Plugins, Skills:
		return true
	}
	return false
}

func managedNodeSelect(c *ui.Context, m *Model, target *string) bool {
	if !m.Live || m.ProviderActions.Hosts == nil {
		return false
	}
	hosts := m.ProviderActions.Hosts()
	if len(hosts) == 0 || len(hosts) == 1 && *target == "" {
		return false
	}
	options := make([]kit.SelectOption, 0, len(hosts))
	for i, host := range hosts {
		label := i18n.T(c, host.Name)
		if label == "" {
			label = host.Node
		}
		value := host.Node
		// The first option is the local Node. The empty value is also the
		// initial choice when no host has been selected.
		if i == 0 {
			value = ""
		}
		options = append(options, kit.SelectOption{Value: value, Label: label, Disabled: !host.Connected})
	}
	title := i18n.T(c, "project_host")
	_, changed := kit.SelectField(c, "managed-node", target, kit.SelectProps{
		Label: title, Placeholder: i18n.T(c, "unavailable"), Options: options,
	})
	return changed
}
