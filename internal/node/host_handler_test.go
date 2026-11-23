package node

import (
	"database/sql"
	"encoding/json"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestHostHandlerUsesCapturedPackageAndOriginalReceipt(t *testing.T) {
	t.Parallel()
	node := nodeFixture(t)
	var info protocol.PluginInfo
	if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
		value, err := store.ReadPlugin(t.Context(), db, "reminders")
		if err == nil && value != nil {
			info = *value
		}
		return err
	}); err != nil || info.Extension == nil {
		t.Fatalf("bundled reminders unavailable: %v", err)
	}
	scope := protocol.DefaultPluginContext()
	scope.Package = plugins.PackageReference(info)
	id := protocol.NewRequestID().String()
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "call_host_handler", Data: protocol.HostHandlerCall{
		Handler: "save", Input: map[string]any{
			"id": id, "project": nil, "title": "Captured host fixture", "message": "",
			"revision": "0", "due_ms": nil, "completed": false,
		},
	}})
	request.Plugin = &scope
	first := dispatchFixture(t, node, node.ID(), request)
	if first.Fault != nil {
		t.Fatal(first.Fault)
	}
	var result struct {
		Ok struct {
			Revision string `json:"revision"`
		} `json:"Ok"`
		Err *protocol.Fault `json:"Err"`
	}
	body, _ := json.Marshal(first.Output.Data)
	if err := json.Unmarshal(body, &result); err != nil || result.Err != nil || result.Ok.Revision != "1" {
		t.Fatalf("host handler failed: %s %v", body, err)
	}
	second := dispatchFixture(t, node, node.ID(), request)
	replayed, _ := json.Marshal(second.Output)
	original, _ := json.Marshal(first.Output)
	if second.Fault != nil || string(replayed) != string(original) {
		t.Fatalf("host handler replay changed result: %+v", second)
	}
}

func TestHostHandlerRejectsUndeclaredAndMismatchedScope(t *testing.T) {
	t.Parallel()
	reference := protocol.PluginReference{Name: "fixture", Digest: strings.Repeat("a", 64)}
	scope := protocol.DefaultPluginContext()
	scope.Package = reference
	info := protocol.PluginInfo{}
	info.Summary.Name, info.Summary.Digest = reference.Name, reference.Digest
	request := protocol.NewRequest(protocol.NodeID{1}, protocol.Command{
		Kind: "call_host_handler", Data: protocol.HostHandlerCall{Handler: "undeclared", Input: nil},
	})
	request.Plugin = &scope
	for _, capture := range []*protocol.ResourceScope{nil, {Plugin: &info, Origin: &scope}} {
		if err := checkHostHandlerScope(request, capture); err == nil {
			t.Fatal("undeclared host callback was accepted")
		}
	}
}
