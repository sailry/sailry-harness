package client

import (
	"encoding/json"
	"strconv"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCopyValueDetachesNestedProtocolData(t *testing.T) {
	type fixture struct {
		Parts [1]any
		Empty []string
		Nil   []byte
	}
	bytes := json.RawMessage("payload")
	original := fixture{Parts: [1]any{map[string][]*json.RawMessage{"result": {&bytes}}}, Empty: []string{}}
	copied := copyValue(original)
	values := copied.Parts[0].(map[string][]*json.RawMessage)
	(*values["result"][0])[0] = 'P'
	values["result"][0] = nil
	delete(values, "result")
	if string(bytes) != "payload" || original.Parts[0].(map[string][]*json.RawMessage)["result"][0] == nil || copied.Empty == nil || copied.Nil != nil {
		t.Fatal("copy shared mutable data or changed a collection's presence")
	}
}

func BenchmarkCopyConversationSnapshot(b *testing.B) {
	snapshot := conversationFixture()
	entry := snapshot.Page.Entries[0]
	for range 1000 {
		snapshot.Page.Entries = append(snapshot.Page.Entries, entry)
	}
	b.ReportAllocs()
	for b.Loop() {
		_ = copyValue(snapshot)
	}
}

func BenchmarkHostCapabilities(b *testing.B) {
	for _, sessions := range []int{0, 1000} {
		b.Run(strconv.Itoa(sessions), func(b *testing.B) {
			snapshot := protocol.DefaultSnapshot()
			snapshot.Sessions = make([]protocol.Session, sessions)
			snapshot.ProjectSources = []string{"local", "clone"}
			snapshot.HostTools = []protocol.HostTool{{Name: "git", Available: true}}
			c := &Client{project: &Projection{snapshot: &snapshot}}
			b.ReportAllocs()
			for b.Loop() {
				c.HostCapabilities()
			}
		})
	}
}
