package node

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/host"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) initHost() {
	node.hostSampler = host.NewSampler()
	node.hostInstalls = host.NewInstallations(host.DeploymentOptions{})
}

func hostCommand(kind string) bool {
	switch kind {
	case "inspect_host", "read_host_metrics", "list_ssh", "save_ssh", "remove_ssh", "trust_ssh", "check_ssh", "install_host", "update_ssh_host", "read_host_install", "cancel_ssh":
		return true
	}
	return false
}

func (node *Node) dispatchHost(ctx context.Context, caller protocol.NodeID, request protocol.Request, body []byte) (link.Admission, error) {
	if request.Plugin != nil {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "plugin invocation authority is unavailable"}
	}
	if err := node.begin(); err != nil {
		return link.Admission{}, err
	}
	completion := make(chan link.Response, 1)
	accepted := link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completion}
	if !accepted.Receipt.Durable {
		go func() {
			defer node.work.Done()
			defer close(completion)
			completion <- node.readHost(ctx, caller, request)
		}()
		return accepted, nil
	}
	var previous *link.Response
	err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		previous, err = admission.Admit(ctx, db, caller, request, body)
		return err
	})
	if err != nil {
		node.work.Done()
		return link.Admission{}, nodeFault(err)
	}
	if previous != nil {
		completion <- *previous
		close(completion)
		node.work.Done()
		return accepted, nil
	}
	go func() {
		defer node.work.Done()
		defer close(completion)
		completion <- node.executeHost(caller, request)
	}()
	return accepted, nil
}

func (node *Node) readHost(ctx context.Context, caller protocol.NodeID, request protocol.Request) link.Response {
	switch request.Command.Kind {
	case "inspect_host":
		value, err := node.hostSampler.Inspect(ctx, node.ID())
		return response(protocol.Output{Kind: "host_info", Data: value}, err)
	case "read_host_metrics":
		input := request.Command.Data.(*protocol.CommandReadHostMetricsPayload)
		value, err := node.hostSampler.Sample(ctx, node.ID(), input.Detailed)
		return response(protocol.Output{Kind: "host_metrics", Data: value}, err)
	case "read_host_install":
		input := request.Command.Data.(*protocol.CommandReadHostInstallPayload)
		value, err := node.hostInstalls.Read(caller, input.Request, request.Plugin)
		return response(protocol.Output{Kind: "host_install_progress", Data: value}, err)
	case "list_ssh":
		var values []protocol.SshProfile
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			values, err = store.ListSsh(ctx, db)
			return err
		})
		return response(protocol.Output{Kind: "ssh_profiles", Data: values}, err)
	}
	return response(protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "Host read expected"})
}

func (node *Node) executeHost(caller protocol.NodeID, request protocol.Request) link.Response {
	var output protocol.Output
	var operationErr error
	configuration := request.Command.Kind == "save_ssh" || request.Command.Kind == "trust_ssh" || request.Command.Kind == "remove_ssh"
	if !configuration {
		output, operationErr = node.runHost(caller, request)
	}
	err := node.profile.Store.Node.Write(node.ctx, func(db *sql.Conn) error {
		var events []protocol.Event
		if configuration {
			if _, err := db.ExecContext(node.ctx, "SAVEPOINT sailry_ssh_configuration"); err != nil {
				return err
			}
			output, events, operationErr = store.ExecuteSshCommand(node.ctx, db, request.Command)
			if operationErr != nil {
				events = nil
				if _, err := db.ExecContext(node.ctx, "ROLLBACK TO sailry_ssh_configuration"); err != nil {
					return err
				}
			}
			if _, err := db.ExecContext(node.ctx, "RELEASE sailry_ssh_configuration"); err != nil {
				return err
			}
			if _, err := store.RecordEvents(node.ctx, db, node.ID(), events); err != nil {
				return err
			}
		}
		return admission.Complete(node.ctx, db, caller, request.ID, response(output, operationErr))
	})
	if err != nil {
		return response(protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "Host operation result could not be recorded; inspect the target using the original request identifier"})
	}
	if configuration {
		node.publish()
	}
	return response(output, operationErr)
}
