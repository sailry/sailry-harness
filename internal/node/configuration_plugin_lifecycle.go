package node

import "github.com/sailry/sailry-harness/internal/protocol"

// Configuration snapshots carry state rather than the source command. Compare
// the receiver's committed settings capture so code and update preferences do
// not interrupt admitted programs; a real settings change still invalidates them.
func preserveFollowedPrograms(previous []protocol.PluginSummary, changes []protocol.Event) map[string]bool {
	settings := make(map[string]uint64, len(previous))
	for _, summary := range previous {
		settings[summary.Name] = summary.SettingsRevision
	}
	preserve := map[string]bool{}
	for _, change := range changes {
		if change.Kind != "plugin_changed" {
			continue
		}
		if summary, ok := dispatchPayload[protocol.PluginSummary](change.Data); ok {
			previous, exists := settings[summary.Name]
			preserve[summary.Name] = exists && previous == summary.SettingsRevision
		}
	}
	return preserve
}
