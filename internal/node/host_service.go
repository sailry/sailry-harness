package node

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/host"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/node/wsl"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) initHost(options Options, database moduleapi.DatabaseExecutor, ssh moduleapi.SSHFactory) {
	node.hostSampler = host.NewSampler()
	var deployment host.DeploymentOptions
	if options.HostDeployment != nil {
		deployment = *options.HostDeployment
	}
	deployment.Release = options.HostRelease
	node.hostInstalls = host.NewInstallations(deployment)
	node.executeDatabase = database
	if ssh != nil {
		node.ssh = ssh(moduleapi.SSHServiceOptions{
			Context: node.ctx, ReadSSH: node.readSSHProfile, WorkspaceRoot: node.workspaceRoot,
			Transfers: node.transfers, Terminals: node.terminals, Installations: node.hostInstalls, Link: node.Link,
		})
	}
	node.wsl = wsl.NewServiceForRelease(options.HostRelease)
}

func hostCommand(kind string) bool {
	switch kind {
	case "inspect_host", "read_host_metrics", "read_plugin_memory", "stop_host_process", "list_ssh", "save_ssh", "remove_ssh", "trust_ssh", "check_ssh", "install_host", "update_ssh_host", "read_host_install", "cancel_ssh":
		return true
	}
	return sshResourceCommand(kind) || databaseCommand(kind) || wslCommand(kind)
}

// dispatchResource runs the SSH resource, external database and WSL commands
// that share the Host admission path. It performs remote work and must be
// called outside any store writer.
func (node *Node) dispatchResource(ctx context.Context, caller protocol.NodeID, request protocol.Request) (bool, protocol.Output, error) {
	if sshResourceCommand(request.Command.Kind) {
		if err := node.checkCommandModule(request.Command); err != nil {
			return true, protocol.Output{}, err
		}
		output, err := node.ssh.Execute(ctx, caller, request)
		return true, output, err
	}
	if handled, output, err := node.dispatchDatabase(ctx, caller, request); handled {
		return true, output, err
	}
	return node.dispatchWSL(ctx, node.wsl, host.ConfigureNetwork, caller, request)
}

func (node *Node) dispatchHost(ctx context.Context, caller protocol.NodeID, request protocol.Request, body []byte) (link.Admission, error) {
	if err := node.checkCommandModule(request.Command); err != nil {
		return link.Admission{}, err
	}
	// Plugin scopes reach only the connection commands their declared database
	// and SSH actions admit; Node inspection and process signals stay with
	// controllers.
	if request.Plugin != nil && (request.Command.Kind == "inspect_host" || request.Command.Kind == "read_host_metrics" || request.Command.Kind == "read_plugin_memory" || request.Command.Kind == "stop_host_process") {
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
		owner := node.commandContext(commandJob{user: userFromContext(ctx), scope: ctx})
		completion <- node.executeHost(owner, caller, request)
	}()
	return accepted, nil
}

func (node *Node) readHost(ctx context.Context, caller protocol.NodeID, request protocol.Request) link.Response {
	if err := node.checkCommandModule(request.Command); err != nil {
		return response(protocol.Output{}, err)
	}
	if handled, output, err := node.dispatchResource(ctx, caller, request); handled {
		return response(output, err)
	}
	switch request.Command.Kind {
	case "inspect_host":
		value, err := node.hostSampler.Inspect(ctx, node.ID())
		if err == nil {
			value.Tools = node.refreshHostTools(ctx)
		}
		return response(protocol.Output{Kind: "host_info", Data: value}, err)
	case "read_host_metrics":
		input := request.Command.Data.(*protocol.CommandReadHostMetricsPayload)
		value, err := node.hostSampler.Sample(ctx, node.ID(), input.Detailed)
		return response(protocol.Output{Kind: "host_metrics", Data: value}, err)
	case "read_plugin_memory":
		value, err := node.readPluginMemory(ctx)
		return response(protocol.Output{Kind: "plugin_memory", Data: value}, err)
	case "read_host_install":
		input := request.Command.Data.(*protocol.CommandReadHostInstallPayload)
		value, err := node.hostInstalls.Read(caller, input.Request, request.Plugin)
		return response(protocol.Output{Kind: "host_install_progress", Data: value}, err)
	case "list_ssh":
		var values []protocol.SshProfile
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			values, err = store.ListSsh(ctx, db)
			if err == nil && request.Plugin != nil {
				values, err = store.FilterPluginSsh(ctx, db, *request.Plugin, values)
			}
			return err
		})
		return response(protocol.Output{Kind: "ssh_profiles", Data: values}, err)
	}
	return response(protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "Host read expected"})
}

func (node *Node) executeHost(ctx context.Context, caller protocol.NodeID, request protocol.Request) link.Response {
	var output protocol.Output
	var operationErr error
	configuration := request.Command.Kind == "save_ssh" || request.Command.Kind == "trust_ssh" || request.Command.Kind == "remove_ssh"
	if !configuration {
		operationErr = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			if err := node.authorizeCommandDB(ctx, db, caller, request); err != nil {
				return err
			}
			return node.validatePluginRequestDB(ctx, db, caller, request)
		})
		if operationErr == nil {
			output, operationErr = node.runHost(ctx, caller, request)
		}
	}
	var events []protocol.Event
	err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		if configuration {
			if _, err := db.ExecContext(ctx, "SAVEPOINT sailry_ssh_configuration"); err != nil {
				return err
			}
			operationErr = node.authorizeCommandDB(ctx, db, caller, request)
			if operationErr == nil {
				operationErr = node.validatePluginRequestDB(ctx, db, caller, request)
			}
			if operationErr == nil && request.Command.Kind == "save_ssh" {
				operationErr = store.RecordConnectionSave(ctx, db, request)
			}
			if operationErr == nil {
				output, events, operationErr = store.ExecuteSshCommand(ctx, db, request.Command)
			}
			if operationErr != nil {
				events = nil
				if _, err := db.ExecContext(ctx, "ROLLBACK TO sailry_ssh_configuration"); err != nil {
					return err
				}
			}
			if _, err := db.ExecContext(ctx, "RELEASE sailry_ssh_configuration"); err != nil {
				return err
			}
			if _, err := store.RecordEvents(ctx, db, node.ID(), events); err != nil {
				return err
			}
		}
		return admission.Complete(ctx, db, caller, request.ID, response(output, operationErr))
	})
	if err != nil {
		if configuration {
			return node.unapplied(caller, request.ID, err)
		}
		// The operation ran outside the transaction, so its result is what is stored.
		return node.settle(caller, request.ID, response(output, operationErr))
	}
	if configuration && operationErr == nil {
		if profileID, ok := sshConfigurationProfileID(request.Command); ok {
			node.ssh.Invalidate(profileID)
		}
		node.publish()
	}
	return response(output, operationErr)
}
