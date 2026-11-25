package plugins

import (
	"encoding/json"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestNativeReportRequiresDeliveredFault(t *testing.T) {
	const ref = "e11525db-c4b2-45ff-9b82-ff23c41f1b15"
	failure := protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "fixture refusal", ReportRef: ref, ReportOwned: true}
	for _, path := range []string{"read rejection", "prepared completion"} {
		t.Run(path, func(t *testing.T) {
			owner, _ := newHostRequestFixture(t, func(request protocol.Request) (link.Admission, error) {
				if path == "read rejection" {
					return link.Admission{}, &failure
				}
				completed := make(chan link.Response, 1)
				completed <- link.Response{Fault: &failure}
				close(completed)
				return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: true}, Completion: completed}, nil
			})
			owner.native.rememberNodeReport(&link.FaultError{Fault: failure})
			if owner.native.OwnsNodeReport(ref, failure.Code) {
				t.Fatal("a locally constructed fault was accepted")
			}
			var received protocol.Fault
			switch path {
			case "read rejection":
				_, err := owner.read(t.Context(), protocol.Command{Kind: "read_plugin_settings", Data: protocol.CommandReadPluginSettingsPayload{Package: owner.scope().Package}})
				if err == nil || json.Unmarshal([]byte(err.Error()), &received) != nil {
					t.Fatalf("plugin read did not receive a fault: %v", err)
				}
			case "prepared completion":
				result, err := owner.complete(t.Context(), prepareHostRequestSettings(t, owner), true)
				var outcome struct {
					Err *protocol.Fault `json:"Err"`
				}
				if err != nil || json.Unmarshal(result, &outcome) != nil || outcome.Err == nil {
					t.Fatalf("plugin request did not receive a fault: %s, %v", result, err)
				}
				received = *outcome.Err
			}
			if received != failure || !owner.native.OwnsNodeReport(received.ReportRef, received.Code) {
				t.Fatalf("received fault was changed or unrecognized: %+v", received)
			}
			if owner.native.OwnsNodeReport(ref, protocol.ErrorCodeInternal) {
				t.Fatal("the ref was accepted with a different code")
			}
			other := NewNativeBinding(t.Context())
			t.Cleanup(other.close)
			if other.OwnsNodeReport(ref, failure.Code) {
				t.Fatal("another view inherited the ref")
			}
			if err := owner.Close(); err != nil {
				t.Fatal(err)
			}
			if owner.native.OwnsNodeReport(ref, failure.Code) || len(owner.native.state.nodeReports) != 0 {
				t.Fatal("closing the view retained the ref")
			}
		})
	}
}
