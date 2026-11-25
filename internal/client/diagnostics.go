package client

import (
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type deliveredFault struct {
	error
	ref   string
	owned bool
}

func (fault *deliveredFault) Unwrap() error { return fault.error }

func nodeCommandFailure(request protocol.Request, err error) error {
	fault := observerFault(err)
	if fault == nil || fault.ReportRef == "" || fault.Code == protocol.ErrorCodeCancelled {
		return err
	}
	owned := true
	if !fault.ReportOwned && request.Plugin == nil {
		switch request.Command.Kind {
		case "list_directory", "inspect_git", "list_worktrees", "search_files", "discover_models":
			// Legacy Hosts issued refs for these reads but left reporting to Desktop.
			owned = false
		}
	}
	return &deliveredFault{error: err, ref: fault.ReportRef, owned: owned}
}

// DeliveredReport returns the ref of a failure received through a command
// transport and whether the Node owns reporting it. The consumer validates
// the ref before logging it. A locally constructed Fault is not a delivery.
func DeliveredReport(err error) (ref string, owned bool) {
	for err != nil {
		if delivered, ok := err.(*deliveredFault); ok {
			return delivered.ref, delivered.owned
		}
		if group, ok := err.(interface{ Unwrap() []error }); ok {
			causes := group.Unwrap()
			if len(causes) != 1 {
				break
			}
			err = causes[0]
		} else {
			err = errors.Unwrap(err)
		}
	}
	return "", false
}
