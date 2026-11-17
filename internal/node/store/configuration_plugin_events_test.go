package store

import (
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginSnapshotEmitsOnlyChangedPackages(t *testing.T) {
	storage, node, source, ctx := snapshotFixture(t)
	first, second := pluginTestInfo("first-fixture", 'a'), pluginTestInfo("second-fixture", 'b')
	first.Summary.Revision, second.Summary.Revision = 1, 1
	firstRecord := followRecord(t, first.Summary.Name, 1, first, nil)
	secondRecord := followRecord(t, second.Summary.Name, 1, second, nil)
	apply := func(revision uint64, records ...protocol.ConfigurationFollowRecord) []protocol.Event {
		t.Helper()
		manifest := protocol.ConfigurationManifest{Source: source, Category: "plugins", Revision: revision, Records: records}
		pages := snapshotPages(t, manifest)
		var changes []protocol.Event
		for _, page := range pages {
			_, current, err := stageSnapshot(t, storage, ctx, node, source, page)
			if err != nil {
				t.Fatal(err)
			}
			changes = append(changes, current...)
		}
		return changes
	}
	changed := func(events []protocol.Event) []string {
		t.Helper()
		names := []string{}
		for _, event := range events {
			if event.Kind == "plugin_changed" {
				summary, ok := event.Data.(protocol.PluginSummary)
				if !ok {
					t.Fatalf("unexpected plugin event: %#v", event.Data)
				}
				names = append(names, summary.Name)
			}
		}
		return names
	}
	if names := changed(apply(1, firstRecord, secondRecord)); !reflect.DeepEqual(names, []string{first.Summary.Name, second.Summary.Name}) {
		t.Fatalf("initial inventory events: %v", names)
	}
	first.Summary.Pinned, first.Summary.Revision = true, 2
	firstRecord = followRecord(t, first.Summary.Name, 2, first, nil)
	if names := changed(apply(2, firstRecord, secondRecord)); !reflect.DeepEqual(names, []string{first.Summary.Name}) {
		t.Fatalf("pin republished unchanged packages: %v", names)
	}
	secondRecord.Body = nil
	if names := changed(apply(3, firstRecord, secondRecord)); len(names) != 0 {
		t.Fatalf("unchanged full and omitted records republished packages: %v", names)
	}
	events := apply(4, secondRecord)
	if names := changed(events); len(names) != 0 {
		t.Fatalf("removal republished an unchanged package: %v", names)
	}
	removed := false
	for _, event := range events {
		if event.Kind == "plugin_removed" {
			value, ok := event.Data.(protocol.EventPluginRemovedPayload)
			removed = ok && value.Name == first.Summary.Name
		}
	}
	if !removed {
		t.Fatalf("snapshot lost the removed package event: %#v", events)
	}
}
