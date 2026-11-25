package node

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/node/host"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) runHost(ctx context.Context, caller protocol.NodeID, request protocol.Request) (protocol.Output, error) {
	if err := node.checkCommandModule(request.Command); err != nil {
		return protocol.Output{}, err
	}
	if handled, output, err := node.dispatchResource(ctx, caller, request); handled {
		return output, err
	}
	if input, ok := request.Command.Data.(*protocol.CommandStopHostProcessPayload); ok {
		err := host.StopProcess(ctx, input.Pid, input.StartedAtSecs, input.Force)
		return protocol.Output{Kind: "host_process_signalled", Data: protocol.OutputHostProcessSignalledPayload{Pid: input.Pid, StartedAtSecs: input.StartedAtSecs}}, err
	}
	if node.ssh == nil {
		return protocol.Output{}, moduleUnavailable("ssh")
	}
	return node.ssh.Execute(ctx, caller, request)
}

func (node *Node) readSSHProfile(ctx context.Context, id protocol.SshID, revision uint64) (protocol.SshProfile, protocol.SshCredential, error) {
	var profile protocol.SshProfile
	var credential protocol.SshCredential
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		profile, credential, err = store.ReadSsh(ctx, db, id, revision)
		return err
	})
	return profile, credential, err
}

func (node *Node) closeSSH() error {
	if node.ssh == nil {
		return nil
	}
	return node.ssh.Close()
}

func sshConfigurationProfileID(command protocol.Command) (protocol.SshID, bool) {
	switch data := command.Data.(type) {
	case *protocol.CommandSaveSshPayload:
		return data.Profile.ID, true
	case protocol.CommandSaveSshPayload:
		return data.Profile.ID, true
	case *protocol.CommandTrustSshPayload:
		return data.Profile, true
	case protocol.CommandTrustSshPayload:
		return data.Profile, true
	case *protocol.CommandRemoveSshPayload:
		return data.Profile, true
	case protocol.CommandRemoveSshPayload:
		return data.Profile, true
	default:
		return protocol.SshID{}, false
	}
}
