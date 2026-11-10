package node

import (
	"database/sql"
	"errors"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/ssh"
)

func (node *Node) runHost(caller protocol.NodeID, request protocol.Request) (protocol.Output, error) {
	if handled, output, err := node.dispatchResource(node.ctx, caller, request); handled {
		return output, err
	}
	if input, ok := request.Command.Data.(*protocol.CommandCancelSshPayload); ok {
		err := node.hostInstalls.Cancel(caller, input.Request, request.Plugin)
		return protocol.Output{Kind: "ssh_cancelled", Data: protocol.OutputSshCancelledPayload{Request: input.Request}}, err
	}
	var id protocol.SshID
	var revision uint64
	var expected *protocol.NodeID
	var network *protocol.NetworkSelection
	switch input := request.Command.Data.(type) {
	case *protocol.CommandCheckSshPayload:
		id, revision = input.Profile, input.ExpectedRevision
	case *protocol.CommandInstallHostPayload:
		id, revision, network = input.Profile, input.ExpectedRevision, input.Network
	case *protocol.CommandUpdateSshHostPayload:
		id, revision = input.Profile, input.ExpectedRevision
		expected = &input.Node
	default:
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "SSH operation expected"}
	}
	if request.Command.Kind != "check_ssh" && node.Link() == nil {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Node Link is unavailable"}
	}
	var profile protocol.SshProfile
	var credential protocol.SshCredential
	if err := node.profile.Store.Node.Read(node.ctx, func(db *sql.Conn) error {
		var err error
		profile, credential, err = store.ReadSsh(node.ctx, db, id, revision)
		return err
	}); err != nil {
		return protocol.Output{}, err
	}
	scoped, finish, err := node.hostInstalls.Begin(node.ctx, caller, request)
	if err != nil {
		return protocol.Output{}, err
	}
	defer finish()
	connection, err := ssh.Connect(scoped, profile, credential)
	if err != nil {
		var required *ssh.HostKeyRequired
		if errors.As(err, &required) {
			return protocol.Output{Kind: "ssh_outcome", Data: required.Outcome()}, nil
		}
		return protocol.Output{}, err
	}
	defer connection.Close()
	if request.Command.Kind == "check_ssh" {
		return protocol.Output{Kind: "ssh_outcome", Data: protocol.SshOutcome{Kind: "connected"}}, nil
	}
	outcome, err := node.hostInstalls.Execute(scoped, connection, node.Link(), caller, request.ID, expected, network)
	return protocol.Output{Kind: "ssh_outcome", Data: outcome}, err
}
