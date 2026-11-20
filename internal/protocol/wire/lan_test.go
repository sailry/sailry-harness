package wire

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestInspectionIgnoresUnusableLANHints(t *testing.T) {
	for _, hints := range []string{`{"unknown":true}`, `["` + strings.Repeat("a", 4096) + `"]`, `[` + strings.Repeat(`"addr",`, MaxLANListeners) + `"addr"]`} {
		var reply Reply
		input := `{"kind":"endpoint","data":{"execution":true,"platform":"linux","lan_listeners":` + hints + `}}`
		if err := json.Unmarshal([]byte(input), &reply); err != nil {
			t.Fatal(err)
		}
		if reply.Endpoint == nil || !reply.Endpoint.Execution || reply.Endpoint.Platform != "linux" || len(reply.Endpoint.LANListeners) != 0 {
			t.Fatalf("inspection: %#v", reply.Endpoint)
		}
	}
}
