package node

import (
	"context"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/wsl"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func wslCommand(kind string) bool {
	switch kind {
	case "list_wsl_distributions", "install_wsl_host", "read_wsl_host_install", "cancel_wsl_host_install":
		return true
	}
	return false
}

// dispatchWSL executes a WSL command inside the Host command path, which owns
// admission, the durable request record and its completion. The context
// bounds the operation: the caller's for reads, the Node's for an installation.
// The Node passes its one installation service and the Host deployment's
// network confirmation, which SSH installation shares.
func (node *Node) dispatchWSL(ctx context.Context, service *wsl.Service, configure wsl.ConfigureNetwork, caller protocol.NodeID, request protocol.Request) (bool, protocol.Output, error) {
	if !wslCommand(request.Command.Kind) {
		return false, protocol.Output{}, nil
	}
	if request.Command.Kind == "list_wsl_distributions" {
		values, err := service.List(ctx, node.environment.TerminalValues())
		return true, protocol.Output{Kind: "wsl_distributions", Data: values}, err
	}
	if request.Plugin != nil {
		return true, protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "WSL Host setup requires a controller"}
	}
	switch input := request.Command.Data.(type) {
	case *protocol.CommandReadWslHostInstallPayload:
		value, err := service.Progress(caller, input.Request)
		return true, protocol.Output{Kind: "host_install_progress", Data: value}, err
	case *protocol.CommandCancelWslHostInstallPayload:
		err := service.Cancel(caller, input.Request)
		return true, protocol.Output{Kind: "wsl_host_install_cancelled", Data: protocol.OutputWslHostInstallCancelledPayload{Request: input.Request}}, err
	case *protocol.CommandInstallWslHostPayload:
		outcome, err := node.installWSLHost(ctx, service, configure, caller, request, input)
		return true, protocol.Output{Kind: "host_installation", Data: outcome}, err
	}
	return true, protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "WSL command expected"}
}

func (node *Node) installWSLHost(ctx context.Context, service *wsl.Service, configure wsl.ConfigureNetwork, caller protocol.NodeID, request protocol.Request, input *protocol.CommandInstallWslHostPayload) (protocol.HostInstallationOutcome, error) {
	if input.Network != nil {
		if _, err := link.NetworkRelayURLs(ctx, *input.Network); err != nil {
			return protocol.HostInstallationOutcome{}, err
		}
	}
	endpoint := node.Link()
	if endpoint == nil {
		return protocol.HostInstallationOutcome{}, unavailable()
	}
	installation, err := service.Begin(ctx, caller, request)
	if err != nil {
		return protocol.HostInstallationOutcome{}, err
	}
	defer installation.Close()
	return installation.Execute(node.environment.TerminalValues(), endpoint, node.ID(), input.Network, configure)
}
