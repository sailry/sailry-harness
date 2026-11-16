package client

import (
	"context"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// SyncIdentity checks durable, authenticated Link relationships before importing
// a source directory or accepting its sponsorship. A ticket's editable contents
// cannot establish that proof. Each host remains a direct Client connection.
func SyncIdentity(ctx context.Context, endpoint *link.Link, target *Client) error {
	address, err := endpoint.Address(ctx)
	if err != nil {
		return err
	}
	output, err := target.Do(ctx, protocol.Command{Kind: "identity", Data: protocol.IdentityCommand{Kind: "read_identity"}})
	if err != nil {
		return err
	}
	state, err := payload[protocol.IdentityState](output.Data)
	if output.Kind != "identity_state" || err != nil {
		return fault(protocol.ErrorCodeInvalidRequest, "invalid identity response")
	}
	for _, sponsorship := range state.Sponsorships {
		if sponsorship.Device != address.ID || sponsorship.State != "pending" {
			continue
		}
		verified, err := endpoint.MyDevice(ctx, sponsorship.Sponsor)
		if err != nil {
			return err
		}
		if !verified {
			continue
		}
		// The sponsor's immutable request ID is also the target device's
		// confirmation ID. Ledgers are caller-scoped, so it survives restart
		// without inventing a new identity for an uncertain merge.
		request := protocol.Request{Version: protocol.VERSION, ID: sponsorship.Request, Target: target.Target(), Command: protocol.Command{Kind: "identity", Data: protocol.IdentityCommand{Kind: "confirm_sponsorship", Data: protocol.ConfirmSponsorship{Sponsor: sponsorship.Sponsor, Request: sponsorship.Request}}}}
		resolution, err := target.Resolve(ctx, request)
		if err != nil {
			return err
		}
		switch resolution.State {
		case RequestAbsent:
			if _, err := target.Settle(ctx, request); err != nil {
				return err
			}
		case RequestApplied:
		default:
			if _, err := resolution.Result(); err != nil {
				return err
			}
		}
	}
	verified, err := endpoint.MyDevice(ctx, target.Target())
	if err != nil || !verified {
		return err
	}
	output, err = target.Do(ctx, protocol.Command{Kind: "identity", Data: protocol.IdentityCommand{Kind: "read_host_directory"}})
	if err != nil {
		return err
	}
	directory, err := payload[protocol.HostDirectory](output.Data)
	if output.Kind != "host_directory" || err != nil || directory.Source != target.Target() {
		return fault(protocol.ErrorCodeInvalidRequest, "invalid host directory response")
	}
	return endpoint.ApplyDirectory(ctx, directory)
}
