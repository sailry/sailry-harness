package node

import (
	"context"
	"database/sql"
	"errors"
	"math"
	"strings"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/ssh"
	cryptossh "golang.org/x/crypto/ssh"
)

const sshFileTimeout = 300 * time.Second

func sshResourceCommand(kind string) bool {
	switch kind {
	case "browse_ssh_directory", "download_ssh_file", "stage_ssh_upload", "finish_ssh_upload", "modify_ssh_file", "run_ssh", "transfer_ssh", "cancel_ssh":
		return true
	}
	return false
}

// sshResource is one validated operation against a saved profile.
type sshResource struct {
	profile  protocol.SshID
	revision uint64
	timeout  time.Duration
	root     string
}

// dispatchSSHResource executes one SSH resource command to completion. It must
// run outside any store writer transaction: it reads the saved profile, then
// connects and performs remote work. ctx is the operation's lifetime (the Node
// lifetime for durable commands); cancel_ssh from the same caller and plugin
// scope stops it early. Effects that may exist after cancellation, timeout or
// a lost connection are reported as OutcomeUnknown and are never replayed.
func (node *Node) dispatchSSHResource(ctx context.Context, caller protocol.NodeID, request protocol.Request) (handled bool, output protocol.Output, err error) {
	if !sshResourceCommand(request.Command.Kind) {
		return false, protocol.Output{}, nil
	}
	switch input := request.Command.Data.(type) {
	case *protocol.CommandCancelSshPayload:
		err = node.hostInstalls.Cancel(caller, input.Request, request.Plugin)
		return true, protocol.Output{Kind: "ssh_cancelled", Data: protocol.OutputSshCancelledPayload{Request: input.Request}}, err
	case *protocol.SshUploadSpec:
		if _, _, err = node.readSSH(ctx, input.Profile, input.ExpectedRevision); err != nil {
			return true, protocol.Output{}, err
		}
		upload, err := node.transfers.StageSSHUpload(ctx, caller, *input)
		return true, protocol.Output{Kind: "ssh_upload", Data: upload}, err
	}
	outcome, err := node.runSSHResource(ctx, caller, request)
	return true, protocol.Output{Kind: "ssh_outcome", Data: outcome}, err
}

func (node *Node) readSSH(ctx context.Context, id protocol.SshID, revision uint64) (protocol.SshProfile, protocol.SshCredential, error) {
	var profile protocol.SshProfile
	var credential protocol.SshCredential
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		profile, credential, err = store.ReadSsh(ctx, db, id, revision)
		return err
	})
	return profile, credential, err
}

func sshTimeout(milliseconds uint64) (time.Duration, bool) {
	if milliseconds == 0 || milliseconds > math.MaxInt64/uint64(time.Millisecond) {
		return 0, false
	}
	return time.Duration(milliseconds) * time.Millisecond, true
}

// prepareSSHResource validates a command before any storage or network use.
func (node *Node) prepareSSHResource(ctx context.Context, command protocol.Command) (sshResource, error) {
	resource := sshResource{timeout: sshFileTimeout}
	var err error
	valid := true
	switch input := command.Data.(type) {
	case *protocol.CommandModifySshFilePayload:
		resource.profile, resource.revision = input.Profile, input.ExpectedRevision
		err = ssh.ValidatePath(input.Path)
		switch action := input.Action.Data.(type) {
		case *protocol.SshFileActionCopyPayload:
			if err == nil {
				err = ssh.ValidatePath(action.Destination)
			}
		case *protocol.SshFileActionMovePayload:
			if err == nil {
				err = ssh.ValidatePath(action.Destination)
			}
		}
	case *protocol.CommandBrowseSshDirectoryPayload:
		resource.profile, resource.revision = input.Profile, input.ExpectedRevision
		err = ssh.ValidatePath(input.Path)
	case *protocol.CommandDownloadSshFilePayload:
		resource.profile, resource.revision = input.Profile, input.ExpectedRevision
		err = ssh.ValidatePath(input.Path)
	case *protocol.CommandFinishSshUploadPayload:
		resource.profile, resource.revision = input.Profile, input.ExpectedRevision
		err = ssh.ValidatePath(input.Path)
	case *protocol.CommandRunSshPayload:
		resource.profile, resource.revision = input.Profile, input.ExpectedRevision
		resource.timeout, valid = sshTimeout(input.TimeoutMs)
		if !valid || strings.TrimSpace(input.Command) == "" {
			err = &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid SSH command or timeout"}
		}
	case *protocol.CommandTransferSshPayload:
		resource.profile, resource.revision = input.Profile, input.ExpectedRevision
		if err = ssh.ValidateRemotePath(input.Transfer.RemotePath); err != nil {
			break
		}
		if resource.timeout, valid = sshTimeout(input.TimeoutMs); !valid {
			err = &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid SSH transfer timeout"}
			break
		}
		resource.root, err = node.workspaceRoot(ctx, input.Transfer.Worktree, false)
	default:
		err = &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "SSH operation expected"}
	}
	return resource, err
}

func (node *Node) runSSHResource(ctx context.Context, caller protocol.NodeID, request protocol.Request) (protocol.SshOutcome, error) {
	resource, err := node.prepareSSHResource(ctx, request.Command)
	if err != nil {
		return protocol.SshOutcome{}, err
	}
	profile, credential, err := node.readSSH(ctx, resource.profile, resource.revision)
	if err != nil {
		return protocol.SshOutcome{}, err
	}
	scoped, finish, err := node.hostInstalls.Begin(ctx, caller, request)
	if err != nil {
		return protocol.SshOutcome{}, err
	}
	defer finish()
	connection, err := ssh.ConnectWithRecovery(scoped, clockwork.NewRealClock(), profile, credential)
	if err != nil {
		var required *ssh.HostKeyRequired
		if errors.As(err, &required) {
			return required.Outcome(), nil
		}
		if scoped.Err() != nil {
			// Nothing was authenticated, so no remote effect can exist.
			return protocol.SshOutcome{}, &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "SSH connection cancelled"}
		}
		return protocol.SshOutcome{}, err
	}
	defer connection.Close()
	timed, cancel := context.WithTimeout(scoped, resource.timeout)
	defer cancel()
	// The SFTP client has no per-request cancellation; closing the connection
	// releases every blocked remote call.
	stop := context.AfterFunc(timed, func() { _ = connection.Close() })
	defer stop()

	var outcome protocol.SshOutcome
	switch input := request.Command.Data.(type) {
	case *protocol.CommandRunSshPayload:
		outcome, err = ssh.Run(timed, connection, input.Command)
		if err != nil {
			err = sshInterrupted(ctx, timed, err, "SSH command cancelled; inspect the remote process before retrying", "SSH connection closed; inspect the remote process before retrying", "SSH command timed out; inspect the remote process before retrying")
		}
	case *protocol.CommandTransferSshPayload:
		var bytes uint64
		bytes, err = node.transfers.TransferSSH(timed, connection, resource.root, core.FileAccessForRequest(request, false), input.Transfer)
		outcome = protocol.SshOutcome{Kind: "transferred", Data: protocol.SshOutcomeTransferredPayload{Bytes: bytes}}
		if err != nil {
			const unconfirmed = "SSH file transfer could not be confirmed; inspect the destination before retrying"
			err = sshInterrupted(ctx, timed, err, unconfirmed+" (outer cancelled)", unconfirmed+" (outer closed)", unconfirmed+" (outer timeout)")
		}
	default:
		outcome, err = node.runSSHFiles(timed, caller, connection, profile, request.Command)
		if err != nil {
			err = sshInterrupted(ctx, timed, err, "SSH file operation cancelled; inspect the remote target before retrying", "SSH file operation interrupted; inspect the remote target before retrying", "SSH file operation timed out; inspect the remote target before retrying")
		}
	}
	if err != nil {
		return protocol.SshOutcome{}, err
	}
	return outcome, nil
}

// runSSHFiles performs one SFTP operation on an authenticated connection.
func (node *Node) runSSHFiles(ctx context.Context, caller protocol.NodeID, connection *cryptossh.Client, profile protocol.SshProfile, command protocol.Command) (protocol.SshOutcome, error) {
	client, err := ssh.OpenSFTP(connection)
	if err != nil {
		return protocol.SshOutcome{}, err
	}
	defer client.Close()
	switch input := command.Data.(type) {
	case *protocol.CommandModifySshFilePayload:
		return protocol.SshOutcome{Kind: "files_changed"}, ssh.Modify(client, input.Path, input.Action)
	case *protocol.CommandBrowseSshDirectoryPayload:
		directory, err := ssh.List(client, input.Path, input.After)
		return protocol.SshOutcome{Kind: "directory", Data: directory}, err
	case *protocol.CommandDownloadSshFilePayload:
		download, err := node.transfers.DownloadSSH(ctx, caller, client, input.Path)
		return protocol.SshOutcome{Kind: "download", Data: download}, err
	case *protocol.CommandFinishSshUploadPayload:
		bytes, err := node.transfers.FinishSSHUpload(caller, client, profile, input.Path, input.Stream)
		return protocol.SshOutcome{Kind: "transferred", Data: protocol.SshOutcomeTransferredPayload{Bytes: bytes}}, err
	}
	return protocol.SshOutcome{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "SSH operation expected"}
}

// sshInterrupted replaces the failure of an operation whose context ended with
// the matching uncertain outcome; an operation that failed on its own keeps
// its fault. parent is the Node-owned lifetime and timed the operation deadline.
func sshInterrupted(parent, timed context.Context, failure error, cancelled, closed, timedOut string) error {
	message := cancelled
	switch {
	case timed.Err() == nil:
		return failure
	case parent.Err() != nil:
		message = closed
	case errors.Is(timed.Err(), context.DeadlineExceeded):
		message = timedOut
	}
	return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: message}
}
