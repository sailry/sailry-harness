package live

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestOfferLocalUpdate covers what a package's remembered directory may
// offer as its update: only a strictly newer version of the same package.
func TestOfferLocalUpdate(t *testing.T) {
	t.Parallel()
	version := func(value string) *string { return &value }
	read := func(name, value string) localPackage {
		local := localPackage{directory: "/work/notes", source: protocol.PluginUploadSourceDirectory}
		local.info.Summary.Name, local.info.Summary.Version = name, version(value)
		local.info.Summary.Compatibility = protocol.PluginCompatibility{Kind: "compatible"}
		return local
	}
	report := func() *protocol.PluginUpdatesReport {
		value := protocol.DefaultPluginUpdatesReport()
		value.Current.Name, value.Current.Version, value.Manual = "notes", version("0.1.0"), true
		return &value
	}
	t.Run("a newer version is the update", func(t *testing.T) {
		current := report()
		offered, err := offerLocalUpdate(current, read("notes", "0.1.1"))
		candidate, valid := detail[protocol.PluginInfo](current.Available.Data)
		if err != nil || !offered || current.Manual || !valid || *candidate.Summary.Version != "0.1.1" {
			t.Fatalf("offered %v, %v, report %+v", offered, err, current)
		}
	})
	t.Run("the same or an older version is not", func(t *testing.T) {
		for _, value := range []string{"0.1.0", "0.1.0+build", "0.0.9"} {
			current := report()
			if offered, err := offerLocalUpdate(current, read("notes", value)); offered || err != nil || current.Available != nil || current.Blocked != nil {
				t.Fatalf("%s: offered %v, %v, report %+v", value, offered, err, current)
			}
		}
	})
	t.Run("a version this Sailry cannot run is blocked", func(t *testing.T) {
		current, local := report(), read("notes", "0.2.0")
		local.info.Summary.Compatibility = protocol.PluginCompatibility{Kind: "requires_newer", Data: protocol.PluginCompatibilityRequiresNewerPayload{Required: "9.0.0"}}
		offered, err := offerLocalUpdate(current, local)
		if offered || err != nil || current.Available != nil || current.Blocked == nil || current.Blocked.Version != "0.2.0" || current.Blocked.Required != "9.0.0" {
			t.Fatalf("offered %v, %v, report %+v", offered, err, current)
		}
	})
	t.Run("another package or an archive is refused", func(t *testing.T) {
		archive := read("notes", "0.1.1")
		archive.directory = ""
		for _, local := range []localPackage{read("other", "0.1.1"), archive} {
			current := report()
			if offered, err := offerLocalUpdate(current, local); offered || err == nil || current.Available != nil || !current.Manual {
				t.Fatalf("offered %v, %v, report %+v", offered, err, current)
			}
		}
	})
}
