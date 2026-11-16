package protocol

import (
	"encoding/json"
	"testing"
)

func TestConfigurationFollowCommandClassification(t *testing.T) {
	t.Parallel()
	for _, action := range []string{"read", "manifest", "package_state", "cache_package", "snapshot_state", "apply_page", "apply", "set_source", "set_target", "mcp_local", "detach", "sync"} {
		command := Command{Kind: "configuration_follow", Data: &ConfigurationFollowCommand{Action: action}}
		expected := action != "read" && action != "manifest" && action != "package_state" && action != "cache_package" && action != "snapshot_state" && action != "apply_page"
		if command.Durable() != expected {
			t.Fatalf("%s durable=%v", action, command.Durable())
		}
		data, err := json.Marshal(command)
		if err != nil {
			t.Fatal(err)
		}
		var decoded Command
		if err := json.Unmarshal(data, &decoded); err != nil {
			t.Fatal(err)
		}
		if decoded.Durable() != expected {
			t.Fatalf("decoded %s durable=%v", action, decoded.Durable())
		}
	}
	var command Command
	if err := json.Unmarshal([]byte(`{"kind":"configuration_follow","data":{"action":"read","future":true}}`), &command); err != nil {
		t.Fatal(err)
	}
}
