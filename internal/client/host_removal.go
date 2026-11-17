package client

import (
	"context"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// RemoveHost removes local trust without waiting for a remote host. Only an
// already live connection receives a bounded background self-revocation. Link
// retains the original request and outcome without reconnecting or retrying.
func RemoveHost(ctx context.Context, endpoint *link.Link, address link.EndpointAddress, local func(context.Context) error) (link.HostRemovalAttempt, error) {
	var attempt link.HostRemovalAttempt
	var notice *link.HostRemovalNotice
	if endpoint.Direction(address.ID).AllowsOutbound() {
		var err error
		attempt, notice, err = endpoint.PrepareHostRemovalNotice(ctx, address.ID)
		if err != nil {
			attempt.State = "failed"
			attempt.Fault = &protocol.Fault{Code: faultCode(err), Message: err.Error()}
		}
	}
	if err := local(context.WithoutCancel(ctx)); err != nil {
		if notice != nil {
			notice.Close()
		}
		return attempt, err
	}
	if notice != nil {
		attempt.State = "unknown"
		if err := notice.Start(); err != nil {
			notice.Close()
			attempt.Fault = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "host removal notification could not start"}
		}
	}
	return attempt, nil
}
