package memorynode_test

import (
	"context"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	memory "github.com/sailry/sailry-harness/modules/memory/node"
	native "github.com/sailry/sailry-harness/sdk/native/node"
	"github.com/sailry/sailry-harness/sdk/native/process"
	"github.com/sailry/sailry-harness/sdk/sdktest"
)

func TestMemoryCatalogLifecycle(t *testing.T) {
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	owner, err := native.StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), native.Options{
		Assembly: native.Assembly{Modules: []native.Module{memory.Module()}},
		MCP:      sdktest.MCPOptions(), Environment: &environment,
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := owner.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})
	call := func(kind string, data any) link.Response {
		t.Helper()
		admitted, err := owner.Dispatch(t.Context(), owner.ID(), protocol.NewRequest(owner.ID(), protocol.Command{Kind: kind, Data: data}))
		if err != nil {
			t.Fatalf("%s: %v", kind, err)
		}
		select {
		case result, ok := <-admitted.Completion:
			if !ok {
				t.Fatalf("%s returned no completion", kind)
			}
			return result
		case <-t.Context().Done():
			t.Fatal(t.Context().Err())
			return link.Response{}
		}
	}
	command := func(kind string, data any) any {
		t.Helper()
		result := call(kind, data)
		if result.Fault != nil {
			t.Fatalf("%s: %v", kind, result.Fault)
		}
		return result.Output.Data
	}
	catalog := func() protocol.MemoryCatalog {
		return command("read_memory_catalog", &protocol.CommandReadMemoryCatalogPayload{View: "active"}).(protocol.MemoryCatalog)
	}
	if before := catalog(); len(before.Entries) != 0 || !before.Settings.Enabled {
		t.Fatalf("initial catalog = %+v", before)
	}
	input := &protocol.CommandSaveMemoryEntryPayload{Title: "Editor style", Body: "Prefer compact tables", Kind: "user"}
	saved := command("save_memory_entry", input).(protocol.MemoryEntry)
	if saved.Summary.ID == "" || saved.Body != input.Body || saved.Summary.Revision == 0 {
		t.Fatalf("saved memory = %+v", saved)
	}
	if listed := catalog().Entries; len(listed) != 1 || listed[0].ID != saved.Summary.ID || listed[0].Title != input.Title {
		t.Fatalf("memory catalog after saving = %+v", listed)
	}
	read := command("read_memory_entry", &protocol.CommandReadMemoryEntryPayload{ID: saved.Summary.ID}).(protocol.MemoryEntry)
	if read.Body != input.Body || read.Summary.Revision != saved.Summary.Revision {
		t.Fatalf("memory after read = %+v", read)
	}
	if snapshot, err := owner.Snapshot(t.Context()); err != nil || snapshot.MemoryRevision == 0 {
		t.Fatalf("saved memory revision did not reach the snapshot: %d, %v", snapshot.MemoryRevision, err)
	}
	input.ID, input.ExpectedRevision, input.Body = &saved.Summary.ID, saved.Summary.Revision, "Prefer compact tables and explicit labels"
	updated := command("save_memory_entry", input).(protocol.MemoryEntry)
	if updated.Summary.ID != saved.Summary.ID || updated.Summary.Revision <= saved.Summary.Revision || updated.Body != input.Body {
		t.Fatalf("updated memory = %+v", updated)
	}
	if stale := call("save_memory_entry", input).Fault; stale == nil || stale.Code != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("stale update = %v", stale)
	}
	command("delete_memory_entry", &protocol.CommandDeleteMemoryEntryPayload{ID: updated.Summary.ID, ExpectedRevision: updated.Summary.Revision})
	if entries := catalog().Entries; len(entries) != 0 {
		t.Fatalf("deleted memory still in catalog: %+v", entries)
	}
}
