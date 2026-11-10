package plugins

import (
	"encoding/json"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func testStatusExtension(t *testing.T) {
	for _, current := range []struct {
		name   string
		change func(*protocol.PluginExtension)
		valid  bool
	}{
		{"button without desktop", func(*protocol.PluginExtension) {}, true},
		{"missing host", func(extension *protocol.PluginExtension) { extension.Host = nil }, false},
		{"missing host Status", func(extension *protocol.PluginExtension) { extension.Host.Status = nil }, false},
		{"desktop scope", func(extension *protocol.PluginExtension) { extension.Scope = protocol.PluginScopeDesktop }, false},
		{"missing Status declaration", func(extension *protocol.PluginExtension) { extension.Ui = nil }, false},
		{"popover without view", func(extension *protocol.PluginExtension) { extension.Ui[0].Kind = protocol.PluginUiKindPopover }, false},
		{"popover with view", func(extension *protocol.PluginExtension) {
			extension.Ui[0].Kind = protocol.PluginUiKindPopover
			extension.Desktop = manifestStatusDesktop()
		}, true},
		{"mixed UI without view", func(extension *protocol.PluginExtension) {
			entry := extension.Ui[0]
			entry.ID, entry.Slot = "context", protocol.PluginUiSlotContext
			extension.Ui = append(extension.Ui, entry)
		}, false},
		{"mixed UI with view", func(extension *protocol.PluginExtension) {
			entry := extension.Ui[0]
			entry.ID, entry.Slot = "context", protocol.PluginUiSlotContext
			extension.Ui = append(extension.Ui, entry)
			extension.Desktop = manifestStatusDesktop()
		}, true},
		{"ordinary UI with view", func(extension *protocol.PluginExtension) {
			extension.Ui[0].Slot = protocol.PluginUiSlotComposer
			extension.Host.Status = nil
			extension.Desktop = manifestStatusDesktop()
		}, true},
		{"ordinary UI without view", func(extension *protocol.PluginExtension) {
			extension.Ui[0].Slot = protocol.PluginUiSlotComposer
			extension.Host.Status = nil
		}, false},
		{"undeclared read", func(extension *protocol.PluginExtension) { extension.Host.Status.Read = "missing" }, false},
		{"mutating read", func(extension *protocol.PluginExtension) { extension.Host.ReadOnly = nil }, false},
		{"optional invoke", func(extension *protocol.PluginExtension) { extension.Host.Status.Invoke = nil }, true},
		{"undeclared invoke", func(extension *protocol.PluginExtension) { extension.Host.Status.Invoke = new("missing") }, false},
		{"read-only invoke", func(extension *protocol.PluginExtension) { extension.Host.Status.Invoke = new("read") }, false},
		{"all changes", func(extension *protocol.PluginExtension) {
			extension.Host.Status.Changes = []protocol.PluginHostChange{protocol.PluginHostChangePackage, protocol.PluginHostChangeConversation, protocol.PluginHostChangeRepository}
		}, true},
		{"duplicate changes", func(extension *protocol.PluginExtension) {
			extension.Host.Status.Changes = []protocol.PluginHostChange{protocol.PluginHostChangePackage, protocol.PluginHostChangePackage}
		}, false},
		{"unknown change", func(extension *protocol.PluginExtension) {
			extension.Host.Status.Changes = []protocol.PluginHostChange{"unknown"}
		}, false},
		{"duplicate UI identity", func(extension *protocol.PluginExtension) { extension.Ui = append(extension.Ui, extension.Ui[0]) }, false},
		{"invalid UI kind", func(extension *protocol.PluginExtension) { extension.Ui[0].Kind = protocol.PluginUiKindMetric }, false},
		{"too many declarations", func(extension *protocol.PluginExtension) {
			extension.Ui = slices.Repeat(extension.Ui, protocol.PluginUiMaxEntries+1)
		}, false},
	} {
		t.Run(current.name, func(t *testing.T) {
			var extension protocol.PluginExtension
			if err := json.Unmarshal([]byte(`{
				"api_version":"v1","scope":"host",
				"host":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],
					"handlers":["read","invoke"],"read_only":["read"],"status":{"read":"read","invoke":"invoke"}},
				"ui":[{"id":"badge","slot":"status","kind":"button","label":{"label":"Badge"},"handler":"badge"}]
			}`), &extension); err != nil {
				t.Fatal(err)
			}
			current.change(&extension)
			if err := validateStatusExtension(extension); (err == nil) != current.valid {
				t.Fatalf("Status extension declaration: %v", err)
			}
		})
	}
}

func manifestStatusDesktop() *protocol.PluginDesktopManifest {
	return &protocol.PluginDesktopManifest{
		UiEntry: new("dev.sailry.platform/view.js"), Resources: []string{"dev.sailry.platform/view.js"},
	}
}
