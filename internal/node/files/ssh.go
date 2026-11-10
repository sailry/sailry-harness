package files

import (
	"context"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/pkg/sftp"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/ssh"
	cryptossh "golang.org/x/crypto/ssh"
)

// StageSSHUpload reserves a caller-owned stream that stages bytes on this Node.
// The remote host is not contacted until FinishSSHUpload publishes them.
func (transfers *Transfers) StageSSHUpload(ctx context.Context, caller protocol.NodeID, spec protocol.SshUploadSpec) (protocol.SshUpload, error) {
	output := protocol.SshUpload{Spec: spec}
	if err := ssh.ValidatePath(spec.Path); err != nil {
		return output, err
	}
	if spec.Size > ssh.TransferLimit {
		return output, invalid("SSH file exceeds the transfer limit")
	}
	if !validRevision(spec.Revision) {
		return output, invalid("invalid upload content revision")
	}
	spec.Revision = strings.ToLower(spec.Revision)
	output.Spec = spec
	entry := &transferEntry{size: spec.Size, revision: spec.Revision, ssh: &spec}
	var err error
	entry.file, entry.tempRoot, entry.temporary, err = transfers.temporary()
	if err == nil {
		err = ctx.Err()
	}
	if err != nil {
		cleanupTransfer(entry)
		return output, err
	}
	output.Stream, err = transfers.reserve(caller, "", entry)
	if err != nil {
		cleanupTransfer(entry)
	}
	return output, err
}

// FinishSSHUpload consumes a completely staged upload exactly once. A request
// naming another caller, profile revision or path leaves the stage untouched;
// once publication starts the stage is gone whatever the remote result.
func (transfers *Transfers) FinishSSHUpload(caller protocol.NodeID, client *sftp.Client, profile protocol.SshProfile, path string, stream protocol.StreamID) (uint64, error) {
	transfers.mu.Lock()
	entry := transfers.entries[stream]
	if entry == nil || entry.caller != caller {
		transfers.mu.Unlock()
		return 0, transferMissing()
	}
	if entry.state == "uploaded" && !entry.expires.After(transfers.now()) {
		delete(transfers.entries, stream)
		transfers.mu.Unlock()
		cleanupTransfer(entry)
		return 0, transferMissing()
	}
	if entry.state != "uploaded" {
		transfers.mu.Unlock()
		return 0, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "upload is not ready for publication"}
	}
	spec := entry.ssh
	if spec == nil || spec.Profile != profile.ID || spec.ExpectedRevision != profile.Revision || spec.Path != path {
		transfers.mu.Unlock()
		return 0, transferMissing()
	}
	entry.state = "finishing"
	if entry.timer != nil {
		entry.timer()
	}
	transfers.workers.Add(1)
	transfers.mu.Unlock()
	defer transfers.workers.Done()
	defer func() {
		transfers.mu.Lock()
		delete(transfers.entries, stream)
		transfers.mu.Unlock()
		cleanupTransfer(entry)
	}()
	return ssh.Upload(client, path, entry.file, entry.size, spec.Overwrite)
}

// DownloadSSH stages a remote file on this Node and serves it through a
// caller-owned, single-use download stream.
func (transfers *Transfers) DownloadSSH(ctx context.Context, caller protocol.NodeID, client *sftp.Client, path string) (protocol.SshDownload, error) {
	output := protocol.SshDownload{Path: path}
	entry := &transferEntry{download: true}
	var err error
	entry.file, entry.tempRoot, entry.temporary, err = transfers.temporary()
	if err == nil {
		entry.size, entry.revision, err = ssh.Download(client, path, entry.file)
	}
	if err == nil {
		err = ctx.Err()
	}
	if err == nil {
		_, err = entry.file.Seek(0, io.SeekStart)
	}
	if err != nil {
		cleanupTransfer(entry)
		return output, err
	}
	output.Size, output.Revision = entry.size, entry.revision
	output.Stream, err = transfers.reserve(caller, "", entry)
	if err != nil {
		cleanupTransfer(entry)
	}
	return output, err
}

// TransferSSH copies one file between a confined worktree path and an explicit
// remote path. Bytes never leave this Node's side of the connection. A download
// is staged in Node storage and published without following links; once
// publication starts it runs to completion regardless of ctx.
func (transfers *Transfers) TransferSSH(ctx context.Context, connection *cryptossh.Client, root string, access core.FileAccess, transfer protocol.SshTransfer) (uint64, error) {
	worktree, err := core.OpenFiles(root, access)
	if err != nil {
		return 0, ssh.AtStage(err, "local preparation")
	}
	// The entry owns the worktree handle, the publication target and staging.
	entry := &transferEntry{files: worktree}
	defer cleanupTransfer(entry)
	var source *os.File
	if transfer.Direction == protocol.SshDirectionUpload {
		source, err = worktree.Open(transfer.Path)
		if err == nil {
			defer source.Close()
		}
	} else {
		entry.target, err = worktree.PrepareUploadTarget(ctx, transfer.Path, nil, ssh.TransferLimit)
		if err == nil {
			entry.file, entry.tempRoot, entry.temporary, err = transfers.temporary()
		}
	}
	if err == nil && transfers.profile != "" && pathWithin(filepath.Join(root, filepath.FromSlash(transfer.Path)), transfers.profile) {
		err = &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "Node storage cannot be transferred over SSH"}
	}
	if err != nil {
		return 0, ssh.AtStage(err, "local preparation")
	}
	client, err := ssh.OpenSFTP(connection)
	if err != nil {
		return 0, ssh.AtStage(err, "remote subsystem")
	}
	defer client.Close()
	if source != nil {
		return ssh.TransferUpload(client, source, transfer.Path, transfer.RemotePath)
	}
	size, revision, err := ssh.TransferDownload(client, transfer.RemotePath, entry.file)
	if err != nil {
		return 0, err
	}
	if err = ctx.Err(); err != nil {
		return 0, err
	}
	if _, err = entry.file.Seek(0, io.SeekStart); err != nil {
		return 0, ssh.AtStage(err, "local publish")
	}
	target := entry.target
	entry.target = nil
	if _, err = target.Write(entry.file, size, revision); err != nil {
		return 0, ssh.AtStage(err, "local publish")
	}
	return size, nil
}

func pathWithin(path, directory string) bool {
	relative, err := filepath.Rel(directory, path)
	return err == nil && relative != ".." && !strings.HasPrefix(relative, ".."+string(filepath.Separator))
}
