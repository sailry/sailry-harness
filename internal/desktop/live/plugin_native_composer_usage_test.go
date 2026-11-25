package live

import (
	"bytes"
	"encoding/json"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginNativeComposerStreamedUsage(t *testing.T) {
	fixture := newNativeComposerFrameFixture(t)
	node, id := fixture.source.snapshot.Node, *fixture.scope.Session
	feed := fixture.root.conversationFeeds[id]
	read := func() (json.RawMessage, string) {
		t.Helper()
		body, err := fixture.native.Operations["sailry/ui.readComposer"](t.Context(), nil)
		if err != nil {
			t.Fatal(err)
		}
		var value struct {
			Cursor string `json:"cursor"`
		}
		if err := json.Unmarshal(body, &value); err != nil {
			t.Fatal(err)
		}
		return body, value.Cursor
	}
	first, cursor := read()
	history := fixture.root.conversationCache[id].composer

	// A streamed view carries its own copy of unchanged statistics.
	feed.latest.Store(nativeComposerFixtureHistory(node, id, 7))
	fixture.window.Frame()
	if current := fixture.root.conversationCache[id].composer; current != history {
		t.Fatal("an equal usage copy did not keep the composer usage projection")
	}
	if body, current := read(); current != cursor || !bytes.Equal(body, first) {
		t.Fatalf("an equal usage copy published the composer again: %s", body)
	}

	feed.latest.Store(nativeComposerFixtureHistory(node, id, 9))
	fixture.window.Frame()
	body, current := read()
	if current == cursor || bytes.Equal(body, first) {
		t.Fatalf("a changed usage was not published: %s", body)
	}
	if fixture.root.conversationCache[id].composer == history {
		t.Fatal("a changed usage kept the previous projection")
	}
}

func TestEqualProtocolValue(t *testing.T) {
	tokens, other := uint64(7), uint64(7)
	base := protocol.DefaultConversationStatistics()
	base.ContextTokens = &tokens
	base.Usage = &protocol.ConversationUsage{Input: 1}
	same := base
	same.ContextTokens = &other
	same.Usage = &protocol.ConversationUsage{Input: 1}
	if !sameComposerStatistics(&base, &same) {
		t.Fatal("equal statistics in separate copies compared different")
	}
	changed := same
	changed.Usage = &protocol.ConversationUsage{Input: 2}
	if sameComposerStatistics(&base, &changed) {
		t.Fatal("a changed usage compared equal")
	}
	if sameComposerStatistics(&base, nil) || !sameComposerStatistics(nil, nil) {
		t.Fatal("absent statistics compared incorrectly")
	}
	compare := func(a, b any) bool { return equalProtocolValue(reflect.ValueOf(&a).Elem(), reflect.ValueOf(&b).Elem()) }
	cases := []struct {
		a, b  any
		equal bool
	}{
		{[]int{}, []int(nil), false},
		{[]int{1, 2}, []int{1, 2}, true},
		{map[string][]int{"a": {1}}, map[string][]int{"a": {1}}, true},
		{map[string][]int{"a": {1}}, map[string][]int{"b": {1}}, false},
		{json.RawMessage(`{"a":1}`), json.RawMessage(`{"a":2}`), false},
		{any(int64(1)), any(uint64(1)), false},
		{func() {}, func() {}, false},
	}
	for index, test := range cases {
		if compare(test.a, test.b) != test.equal {
			t.Errorf("case %d: %#v and %#v compared %v", index, test.a, test.b, !test.equal)
		}
	}
}
