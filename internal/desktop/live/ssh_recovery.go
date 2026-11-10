package live

import (
	"context"
	"errors"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) recoverSSH(operation *sshInstallation) {
	if operation.running {
		return
	}
	if operation.networkFix != nil {
		r.recoverSSHNetwork(operation)
		return
	}
	if operation.partial || operation.request == nil {
		r.enqueue(func(c *ui.Context) {
			r.failSSH(c, operation, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "Host installation must be checked on the target"})
		})
		return
	}
	request := *operation.request
	operation.running = true
	r.async(func() {
		outcome, err := r.controller.Outcome(r.ctx, request)
		r.enqueue(func(c *ui.Context) {
			if operation.request == nil || operation.request.ID != request.ID {
				return
			}
			operation.running = false
			if !operation.active {
				return
			}
			if err != nil {
				r.failSSH(c, operation, err)
				return
			}
			switch outcome.Kind {
			case "completed":
				result, ok := detail[protocol.Result[protocol.Output, protocol.Fault]](outcome.Data)
				if ok && result.Ok != nil {
					r.completeSSH(c, operation, *result.Ok, nil)
					return
				}
				if ok && result.Err != nil {
					operation.partial = result.Err.Code == protocol.ErrorCodeOutcomeUnknown
					r.completeSSH(c, operation, protocol.Output{}, result.Err)
					return
				}
			case "not_admitted":
				operation.running = true
				r.async(func() {
					output, executeErr := r.controller.Execute(r.ctx, request)
					r.enqueue(func(c *ui.Context) {
						operation.running = false
						if operation.active {
							r.completeSSH(c, operation, output, executeErr)
						}
					})
				})
				return
			case "admitted":
				r.failSSH(c, operation, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "Host installation is still running"})
				return
			case "unknown":
				operation.partial = true
				r.failSSH(c, operation, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "Host installation result is unknown"})
				return
			}
			r.failSSH(c, operation, errors.New("Host installation receipt is invalid"))
		})
	})
}

func (r *Runtime) recoverSSHNetwork(operation *sshInstallation) {
	if operation.running {
		return
	}
	fix := *operation.networkFix
	operation.running = true
	r.async(func() {
		err := r.confirmConnectionNetwork(r.ctx, fix.Node, fix.Selection)
		r.enqueue(func(c *ui.Context) {
			operation.running = false
			if !operation.active {
				return
			}
			if err != nil {
				r.failSSH(c, operation, err)
				return
			}
			r.completeSSH(c, operation, protocol.Output{Kind: "ssh_outcome", Data: protocol.SshOutcome{Kind: "host_installed", Data: protocol.SshOutcomeHostInstalledPayload{Node: fix.Node}}}, nil)
		})
	})
}

func (r *Runtime) confirmConnectionNetwork(ctx context.Context, node protocol.NodeID, selection protocol.NetworkSelection) error {
	if r.network == nil || r.network.endpoint == nil {
		return unknownConnectionNetwork()
	}
	endpoint := r.network.endpoint
	peers, err := endpoint.Peers(ctx)
	if err != nil {
		return unknownConnectionNetwork()
	}
	for _, previous := range peers {
		if previous.ID != node {
			continue
		}
		controller := client.New(endpoint.Remote(previous))
		defer controller.Close()
		actual, err := readConnectionNetwork(ctx, controller)
		if err != nil {
			candidate, addressErr := link.CandidateAddress(previous, selection)
			if addressErr != nil {
				return unknownConnectionNetwork()
			}
			if err := endpoint.UpdatePeer(ctx, candidate); err != nil {
				return unknownConnectionNetwork()
			}
			actual, err = readConnectionNetwork(ctx, controller)
		}
		if err != nil || actual.Revision == 0 || !sameConnectionNetwork(actual.Selection, &selection) || !sameConnectionNetwork(actual.Effective, &selection) {
			return unknownConnectionNetwork()
		}
		address, err := connectionNetworkAddress(actual, node)
		if err != nil {
			return unknownConnectionNetwork()
		}
		return endpoint.UpdatePeer(ctx, address)
	}
	return unknownConnectionNetwork()
}
