package ssh

import (
	"encoding/hex"
	"errors"
	"io"
	"io/fs"
	"os"
	"strings"

	"github.com/pkg/sftp"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

// Failure points of a remote read, shared by the browser download and the
// worktree transfer, which report them differently.
const (
	stageOpen    = "remote open"
	stageStat    = "remote stat"
	stageInvalid = "invalid file"
	stageRead    = "remote read"
	stageLimit   = "remote read limit"
	stageLocal   = "local write"
	stageChanged = "source changed"
	stageClose   = "remote close"
)

// sourceUnchanged is the metadata recheck applied after copying a file.
func sourceUnchanged(before, after fs.FileInfo, copied uint64) bool {
	return before.Size() >= 0 && uint64(before.Size()) == copied && before.Size() == after.Size() && before.ModTime().Equal(after.ModTime())
}

// fetch copies one remote regular file into local, hashing it, and names the
// stage of any failure. The remote handle is always released.
func fetch(client *sftp.Client, path string, local io.Writer) (size uint64, revision, stage string, err error) {
	remote, err := client.Open(path)
	if err != nil {
		return 0, "", stageOpen, err
	}
	defer remote.Close()
	before, err := remote.Stat()
	if err != nil {
		return 0, "", stageStat, err
	}
	if !before.Mode().IsRegular() || before.Size() < 0 || before.Size() > TransferLimit {
		return 0, "", stageInvalid, errors.New("not a regular file within the transfer limit")
	}
	hash := blake3.New(32, nil)
	buffer := make([]byte, protocol.FILE_TRANSFER_CHUNK_BYTES)
	for {
		count, readErr := remote.Read(buffer)
		if count > 0 {
			size += uint64(count)
			if size > TransferLimit {
				return 0, "", stageLimit, errors.New("remote file exceeds the transfer limit")
			}
			if _, err = local.Write(buffer[:count]); err != nil {
				return 0, "", stageLocal, err
			}
			_, _ = hash.Write(buffer[:count])
		}
		if errors.Is(readErr, io.EOF) {
			break
		}
		if readErr != nil {
			return 0, "", stageRead, readErr
		}
	}
	after, err := remote.Stat()
	if err != nil {
		return 0, "", stageStat, err
	}
	if !sourceUnchanged(before, after, size) {
		return 0, "", stageChanged, errors.New("remote file changed")
	}
	revision = hex.EncodeToString(hash.Sum(nil))
	if err = remote.Close(); err != nil {
		return size, revision, stageClose, err
	}
	return size, revision, "", nil
}

// Download stages a browsed remote file and returns its size and BLAKE3
// revision. Nothing remote changes, so every failure is definite.
func Download(client *sftp.Client, path string, local io.Writer) (uint64, string, error) {
	size, revision, stage, err := fetch(client, path, local)
	switch stage {
	case "", stageClose:
		// The verified bytes are already staged locally; a lost close reply
		// changes nothing a read-only download depends on.
		return size, revision, nil
	case stageInvalid:
		return 0, "", invalid("Select a regular file within the transfer limit")
	case stageLimit:
		return 0, "", invalid("SSH file exceeds the transfer limit")
	case stageLocal:
		return 0, "", localFault(err)
	case stageChanged:
		return 0, "", &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "Remote file changed during download"}
	}
	return 0, "", sftpFault(err)
}

// TransferDownload copies a remote file for worktree publication. Remote
// failures are uncertain and carry the stage that could not be confirmed.
func TransferDownload(client *sftp.Client, path string, local io.Writer) (uint64, string, error) {
	size, revision, stage, err := fetch(client, path, local)
	switch stage {
	case "":
		return size, revision, nil
	case stageOpen, stageStat:
		return 0, "", remoteFailure(stage, err)
	case stageInvalid:
		return 0, "", AtStage(unknown("SSH server returned an invalid or oversized file"), stageStat)
	case stageLimit:
		return 0, "", Interrupted(stage)
	case stageLocal:
		return 0, "", localFailure(stage, err)
	case stageChanged:
		return 0, "", AtStage(unknown("SSH download source changed; inspect the remote file"), stageStat)
	}
	return 0, "", ioFailure(stage, err)
}

func uploadFlags(overwrite bool) int {
	if overwrite {
		return os.O_WRONLY | os.O_CREATE | os.O_TRUNC
	}
	return os.O_WRONLY | os.O_CREATE | os.O_EXCL
}

// Upload publishes a staged upload to its explicit remote path. An existing
// entry is a conflict unless the stage asked to overwrite it.
func Upload(client *sftp.Client, path string, source io.Reader, size uint64, overwrite bool) (uint64, error) {
	if !overwrite {
		if _, err := client.Lstat(path); err == nil {
			return 0, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "Remote file already exists"}
		}
	}
	remote, err := client.OpenFile(path, uploadFlags(overwrite))
	if err != nil {
		return 0, sftpFault(err)
	}
	copied, err := io.Copy(remote, source)
	if err != nil {
		_ = remote.Close()
		return 0, unknown("SSH upload interrupted; inspect the remote file")
	}
	if err = remote.Close(); err != nil {
		return 0, unknown("SSH upload completion is unavailable; inspect the remote file")
	}
	if uint64(copied) != size {
		return 0, unknown("SSH upload size differs; inspect the remote file")
	}
	return size, nil
}

// uploadDestination places a file inside an existing remote directory.
func uploadDestination(remotePath, localPath string, directory bool) string {
	if !directory {
		return remotePath
	}
	return strings.TrimRight(remotePath, "/") + "/" + localPath[strings.LastIndexByte(localPath, '/')+1:]
}

// TransferUpload copies an opened worktree file to the remote host. The file a
// transfer creates is private to the remote account.
func TransferUpload(client *sftp.Client, source *os.File, localPath, remotePath string) (uint64, error) {
	before, err := source.Stat()
	if err != nil {
		return 0, localFailure("local metadata", err)
	}
	if before.Size() > TransferLimit {
		return 0, invalid("SSH file exceeds the transfer limit")
	}
	metadata, err := client.Stat(remotePath)
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		return 0, remoteFailure(stageStat, err)
	}
	remotePath = uploadDestination(remotePath, localPath, err == nil && metadata.IsDir())
	// The pinned client cannot send creation attributes with the open request.
	// An exclusive create identifies a file this transfer made, which is then
	// restricted before any content is written; an existing file keeps its mode.
	remote, err := createExclusive(client, remotePath)
	if err == nil {
		if err = remote.Chmod(0o600); err != nil {
			_ = remote.Close()
			return 0, remoteFailure(stageOpen, err)
		}
	} else if remote, err = client.OpenFile(remotePath, uploadFlags(true)); err != nil {
		return 0, remoteFailure(stageOpen, err)
	}
	defer remote.Close()
	remaining := uint64(before.Size())
	buffer := make([]byte, protocol.FILE_TRANSFER_CHUNK_BYTES)
	for remaining > 0 {
		count, err := source.Read(buffer[:min(remaining, uint64(len(buffer)))])
		if count == 0 {
			if err == nil || errors.Is(err, io.EOF) {
				return 0, Interrupted("source EOF")
			}
			return 0, ioFailure("local read", err)
		}
		if _, err = remote.Write(buffer[:count]); err != nil {
			return 0, ioFailure("remote write", err)
		}
		remaining -= uint64(count)
	}
	after, err := source.Stat()
	if err != nil {
		return 0, ioFailure("local metadata", err)
	}
	if !sourceUnchanged(before, after, uint64(before.Size())) {
		return 0, AtStage(unknown("SSH upload source changed; inspect the remote file"), "local metadata")
	}
	if err = remote.Close(); err != nil {
		return 0, ioFailure(stageClose, err)
	}
	return uint64(before.Size()), nil
}

// Interrupted reports a transfer whose destination state is unconfirmed.
func Interrupted(stage string) error {
	return AtStage(unknown("SSH file transfer could not be confirmed; inspect the destination before retrying"), stage)
}

// AtStage appends the failing stage to a fault while bounding untrusted text.
func AtStage(err error, stage string) error {
	fault := &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: err.Error()}
	var existing *protocol.Fault
	if errors.As(err, &existing) {
		copied := *existing
		fault = &copied
	}
	fault.Message = bounded(fault.Message) + " (" + stage + ")"
	return fault
}

func remoteFailure(stage string, err error) error {
	// The pinned client reduces these two statuses to the standard sentinels.
	cause := "SFTP: " + bounded(err.Error())
	if errors.Is(err, os.ErrNotExist) {
		cause = "SFTP status=NoSuchFile (2)"
	} else if errors.Is(err, os.ErrPermission) {
		cause = "SFTP status=PermissionDenied (3)"
	}
	return Interrupted(stage + ": " + cause)
}

func ioFailure(stage string, err error) error {
	return Interrupted(stage + ": I/O: " + bounded(err.Error()))
}

func localFailure(stage string, err error) error {
	return AtStage(localFault(err), stage+": I/O")
}

func localFault(err error) *protocol.Fault {
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		return fault
	}
	code := protocol.ErrorCodeInvalidRequest
	if errors.Is(err, fs.ErrNotExist) {
		code = protocol.ErrorCodeNotFound
	} else if errors.Is(err, fs.ErrPermission) {
		code = protocol.ErrorCodePermissionDenied
	}
	return &protocol.Fault{Code: code, Message: "local file operation failed: " + bounded(err.Error())}
}
