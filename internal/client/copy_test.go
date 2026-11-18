package client

import (
	"encoding/json"
	"testing"
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
