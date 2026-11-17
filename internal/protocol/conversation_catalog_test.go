package protocol

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestModelCatalogBundledStatus(t *testing.T) {
	status := DefaultConversationCatalogStatus()
	status.Bundled, status.Providers, status.Models = true, 2, 3
	event := Event{Kind: "model_catalog_changed", Data: status}
	raw, err := json.Marshal(event)
	if err != nil {
		t.Fatal(err)
	}
	var decoded Event
	if err := json.Unmarshal(raw, &decoded); err != nil {
		t.Fatal(err)
	}
	actual, ok := decoded.Data.(*ConversationCatalogStatus)
	if !ok || !actual.Bundled || actual.UpdatedAtMs != nil || actual.Models != 3 {
		t.Fatalf("bundled catalog status lost its source: %+v", decoded)
	}
	if err := json.Unmarshal([]byte(`{"revision":1,"providers":2,"models":3,"bytes":4}`), &status); err != nil {
		t.Fatal(err)
	}
	if status.Bundled {
		t.Fatal("an older cached status retained the bundled source")
	}
	raw, err = json.Marshal(status)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(raw), `"bundled"`) {
		t.Fatal("a downloaded catalog added an unnecessary bundled field")
	}
}
