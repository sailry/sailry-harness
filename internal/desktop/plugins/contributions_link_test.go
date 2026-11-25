package plugins

import (
	"encoding/json"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestContributionLinkRequiresVisibleDeclaredState(t *testing.T) {
	registry, pkg, hooks := contributionFixture(t, `[
		{"id":"link","slot":"composer","kind":"button","label":{"label":"Open"},"handler":"open","intent":"open","link_scheme":"sailry-plugin-fixture"},
		{"id":"ordinary","slot":"composer","kind":"button","label":{"label":"Ordinary"},"handler":"ordinary"}
	]`)
	if err := registry.SetActivity(pkg, true, false); err != nil {
		t.Fatal(err)
	}
	key, ok := registry.Key(pkg, "link")
	if !ok {
		t.Fatal("missing link contribution")
	}
	ordinary, ok := registry.Key(pkg, "ordinary")
	if !ok {
		t.Fatal("missing ordinary contribution")
	}
	invoke := func(path json.RawMessage) error { return registry.Invoke(key, protocol.PluginUiEventKindLink, path) }
	const destination = `"note/Today-2"`
	if err := invoke(json.RawMessage(destination)); err == nil {
		t.Fatal("an unpublished button handled a link")
	}
	contributionPublish(t, hooks, `[{"id":"link","visible":false,"enabled":true},{"id":"ordinary","visible":true,"enabled":true}]`)
	if err := invoke(json.RawMessage(destination)); err == nil {
		t.Fatal("an intent made a hidden button handle a link")
	}
	if err := registry.Invoke(ordinary, protocol.PluginUiEventKindLink, json.RawMessage(destination)); err == nil {
		t.Fatal("a legacy button without a declaration handled a link")
	}
	contributionPublish(t, hooks, `[{"id":"link","visible":true,"enabled":true}]`)
	for _, value := range []string{`null`, `"../note"`, `"notes/./Today"`, `"/etc/passwd"`, `"notes/%2e%2e/Today"`, `"notes/a%2fb"`, `"notes\\Today"`, `"notes?scope=other"`} {
		if err := invoke(json.RawMessage(value)); err == nil {
			t.Fatalf("invalid route was delivered: %s", value)
		}
	}
	if err := invoke(json.RawMessage(destination)); err != nil {
		t.Fatal(err)
	}
	if err := invoke(json.RawMessage(destination)); err == nil {
		t.Fatal("link was invoked twice without acknowledgment")
	}
	event := contributionNext(t, hooks)
	if event.Sequence != 1 || event.ID != "link" || event.Handler != "open" || event.Kind != protocol.PluginUiEventKindLink || string(event.Value) != destination {
		t.Fatalf("link event = %+v, value %s", event, event.Value)
	}
}
