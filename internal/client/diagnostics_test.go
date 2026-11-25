package client

import (
	"errors"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestReadRetainsNodeDiagnostic(t *testing.T) {
	const ref = "e11525db-c4b2-45ff-9b82-ff23c41f1b15"
	failure := protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "Node could not finish the read", ReportRef: ref, ReportOwned: true}
	for _, test := range []struct {
		name       string
		admission  *protocol.Fault
		completion *protocol.Fault
		ref        string
		message    string
	}{
		{name: "admission", admission: &failure, ref: ref, message: failure.Message},
		{name: "completion", completion: &failure, ref: ref, message: failure.Message},
		{name: "lost response", message: "Link connection was lost before the reply"},
	} {
		t.Run(test.name, func(t *testing.T) {
			c := New(&clientTransport{dispatch: func(protocol.Request) (link.Admission, error) {
				if test.admission != nil {
					return link.Admission{}, test.admission
				}
				completion := make(chan link.Response, 1)
				if test.completion != nil {
					completion <- link.Response{Fault: test.completion}
				}
				close(completion)
				return link.Admission{Completion: completion}, nil
			}})
			t.Cleanup(func() { _ = c.Close() })
			_, err := c.read(t.Context(), protocol.Command{Kind: "snapshot"})
			if fault := observerFault(err); fault == nil || fault.Code != protocol.ErrorCodeUnavailable || fault.ReportRef != test.ref || fault.ReportOwned != (test.ref != "") || fault.Message != test.message {
				t.Fatalf("read fault = %+v", fault)
			}
			if ref, owned := DeliveredReport(err); ref != test.ref || owned != (test.ref != "") {
				t.Fatalf("read report = %q, owned=%v", ref, owned)
			}
			if failure.Code != protocol.ErrorCodeOutcomeUnknown {
				t.Fatalf("read rewrote the original fault: %+v", failure)
			}
		})
	}
}

func TestCommandFaultReportOwnership(t *testing.T) {
	const ref = "e11525db-c4b2-45ff-9b82-ff23c41f1b15"
	for _, test := range []struct {
		name, kind            string
		marked, plugin, owned bool
	}{
		{name: "legacy directory", kind: "list_directory"},
		{name: "legacy Git", kind: "inspect_git"},
		{name: "legacy worktrees", kind: "list_worktrees"},
		{name: "legacy search", kind: "search_files"},
		{name: "legacy models", kind: "discover_models"},
		{name: "owned read", kind: "inspect_git", marked: true, owned: true},
		{name: "legacy scoped plugin read", kind: "inspect_git", plugin: true, owned: true},
		{name: "legacy mutation", kind: "write_file", owned: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			for _, completion := range []bool{false, true} {
				failure := protocol.NewConfigurationFault(protocol.ConfigurationCredentialMissing, "fixture credential is unavailable")
				failure.ReportRef, failure.ReportOwned = ref, test.marked
				c := New(&clientTransport{dispatch: func(protocol.Request) (link.Admission, error) {
					if !completion {
						return link.Admission{}, failure
					}
					completed := make(chan link.Response, 1)
					completed <- link.Response{Fault: failure}
					close(completed)
					return link.Admission{Completion: completed}, nil
				}})
				t.Cleanup(func() { _ = c.Close() })
				request := c.Prepare(protocol.Command{Kind: test.kind})
				if test.plugin {
					request.Plugin = new(protocol.PluginContext)
				}
				_, err := c.Execute(t.Context(), request)
				if actual := observerFault(err); actual == nil || *actual != *failure {
					t.Fatalf("completion=%v: changed fault %+v", completion, actual)
				}
				if got, owned := DeliveredReport(fmt.Errorf("operation: %w", err)); got != ref || owned != test.owned {
					t.Fatalf("completion=%v: report %q, owned=%v", completion, got, owned)
				}
				if got, owned := DeliveredReport(errors.Join(err, errors.New("separate local failure"))); got != "" || owned {
					t.Fatalf("completion=%v: a separate local failure inherited report %q", completion, got)
				}
			}
		})
	}
}
