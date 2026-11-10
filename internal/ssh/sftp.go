package ssh

import (
	"context"
	"encoding/binary"
	"errors"
	"io"
	"os"
	"sort"
	"strings"
	"sync"
	"unicode"

	"github.com/pkg/sftp"
	"github.com/sailry/sailry-harness/internal/protocol"
	cryptossh "golang.org/x/crypto/ssh"
)

// TransferLimit bounds every SSH file transfer in either direction.
const TransferLimit = 1 << 30

const outputLimit = 128 * 1024

// ValidatePath accepts an explicit remote destination. Remote paths are not a
// sandbox: the saved profile's account decides what they can reach.
func ValidatePath(path string) error {
	if path == "" || len(path) > 4096 || strings.ContainsRune(path, 0) {
		return invalid("Invalid SSH file path")
	}
	return nil
}

// ValidateRemotePath applies the stricter transfer rule: no control characters.
func ValidateRemotePath(path string) error {
	if path == "" || len(path) > 4096 || strings.ContainsFunc(path, unicode.IsControl) {
		return invalid("invalid SSH remote path")
	}
	return nil
}

// OpenSFTP starts the SFTP subsystem on an authenticated connection.
func OpenSFTP(client *cryptossh.Client) (*sftp.Client, error) {
	files, err := sftp.NewClient(client)
	if err != nil {
		return nil, unavailable("SFTP is unavailable on the SSH server")
	}
	return files, nil
}

func entryBefore(directory bool, name string, otherDirectory bool, otherName string) bool {
	if directory != otherDirectory {
		return directory
	}
	return name < otherName
}

// PageEntries orders directories first, then exact byte order of names, and
// returns the page after the cursor with the continuation when more remain.
func PageEntries(entries []protocol.FileEntry, after *protocol.SshCursor) ([]protocol.FileEntry, *protocol.SshCursor) {
	sort.SliceStable(entries, func(left, right int) bool {
		return entryBefore(entries[left].Kind == protocol.EntryKindDirectory, entries[left].Name, entries[right].Kind == protocol.EntryKindDirectory, entries[right].Name)
	})
	if after != nil {
		start := sort.Search(len(entries), func(index int) bool {
			return entryBefore(after.Directory, after.Name, entries[index].Kind == protocol.EntryKindDirectory, entries[index].Name)
		})
		entries = entries[start:]
	}
	if len(entries) <= protocol.MAX_DIRECTORY_ENTRIES {
		return entries, nil
	}
	entries = entries[:protocol.MAX_DIRECTORY_ENTRIES]
	last := entries[len(entries)-1]
	return entries, &protocol.SshCursor{Directory: last.Kind == protocol.EntryKindDirectory, Name: last.Name}
}

func entryKind(mode os.FileMode) protocol.EntryKind {
	switch {
	case mode.IsDir():
		return protocol.EntryKindDirectory
	case mode.IsRegular():
		return protocol.EntryKindFile
	case mode&os.ModeSymlink != 0:
		return protocol.EntryKindSymlink
	}
	return protocol.EntryKindOther
}

// List reads one sorted page of a canonicalized remote directory.
func List(client *sftp.Client, path string, after *protocol.SshCursor) (protocol.SshDirectory, error) {
	path, err := client.RealPath(path)
	if err != nil {
		return protocol.SshDirectory{}, sftpFault(err)
	}
	listed, err := client.ReadDir(path)
	if err != nil {
		return protocol.SshDirectory{}, sftpFault(err)
	}
	entries := make([]protocol.FileEntry, 0, len(listed))
	for _, entry := range listed {
		if strings.ContainsAny(entry.Name(), "/\x00") {
			continue
		}
		entries = append(entries, protocol.FileEntry{Name: entry.Name(), Kind: entryKind(entry.Mode()), Size: uint64(max(entry.Size(), 0))})
	}
	entries, next := PageEntries(entries, after)
	return protocol.SshDirectory{Path: path, Entries: entries, Next: next}, nil
}

// splitTarget separates the entry a mutation names from its parent directory.
func splitTarget(path string) (parent, name string, err error) {
	if err = ValidatePath(path); err != nil {
		return "", "", err
	}
	path = strings.TrimRight(path, "/")
	parent, name = ".", path
	if index := strings.LastIndexByte(path, '/'); index >= 0 {
		parent, name = path[:index], path[index+1:]
		if parent == "" {
			parent = "/"
		}
	}
	if name == "" || name == "." || name == ".." {
		return "", "", invalid("Select a file or directory")
	}
	return parent, name, nil
}

func joinRemote(parent, name string) string {
	return strings.TrimRight(parent, "/") + "/" + name
}

// nests reports a destination equal to or inside its own source.
func nests(source, destination string) bool {
	return source == destination || strings.HasPrefix(destination, source+"/")
}

func resolveTarget(client *sftp.Client, path string) (string, error) {
	parent, name, err := splitTarget(path)
	if err != nil {
		return "", err
	}
	parent, err = client.RealPath(parent)
	if err != nil {
		return "", sftpFault(err)
	}
	return joinRemote(parent, name), nil
}

func absent(client *sftp.Client, path string) error {
	_, err := client.Lstat(path)
	if err == nil {
		return &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "Remote destination already exists"}
	}
	if fault := sftpFault(err); fault.Code != protocol.ErrorCodeNotFound {
		return fault
	}
	return nil
}

func createExclusive(client *sftp.Client, path string) (*sftp.File, error) {
	return client.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL)
}

// Modify creates, copies, moves or removes one remote entry.
func Modify(client *sftp.Client, path string, action protocol.SshFileAction) error {
	source, err := resolveTarget(client, path)
	if err != nil {
		return err
	}
	destination := ""
	switch input := action.Data.(type) {
	case *protocol.SshFileActionCreatePayload:
		return create(client, source, input.Directory)
	case protocol.SshFileActionCreatePayload:
		return create(client, source, input.Directory)
	case *protocol.SshFileActionCopyPayload:
		destination = input.Destination
	case protocol.SshFileActionCopyPayload:
		destination = input.Destination
	case *protocol.SshFileActionMovePayload:
		destination = input.Destination
	case protocol.SshFileActionMovePayload:
		destination = input.Destination
	}
	switch action.Kind {
	case "remove":
		return remove(client, source)
	case "copy", "move":
	default:
		return invalid("SSH file action expected")
	}
	destination, err = resolveTarget(client, destination)
	if err != nil {
		return err
	}
	if nests(source, destination) {
		return invalid("Cannot place an entry inside itself")
	}
	if err = absent(client, destination); err != nil {
		return err
	}
	if action.Kind == "copy" {
		return copyRemote(client, source, destination)
	}
	if err = client.Rename(source, destination); err != nil {
		return sftpFault(err)
	}
	return nil
}

func create(client *sftp.Client, path string, directory bool) error {
	if err := absent(client, path); err != nil {
		return err
	}
	if directory {
		if err := client.Mkdir(path); err != nil {
			return sftpFault(err)
		}
		return nil
	}
	file, err := createExclusive(client, path)
	if err != nil {
		return sftpFault(err)
	}
	if err = file.Close(); err != nil {
		return sftpFault(err)
	}
	return nil
}

func remove(client *sftp.Client, path string) error {
	type step struct {
		path    string
		visited bool
	}
	pending := []step{{path: path}}
	for len(pending) > 0 {
		current := pending[len(pending)-1]
		pending = pending[:len(pending)-1]
		if current.visited {
			if err := client.RemoveDirectory(current.path); err != nil {
				return sftpFault(err)
			}
			continue
		}
		metadata, err := client.Lstat(current.path)
		if err != nil {
			return sftpFault(err)
		}
		if !metadata.IsDir() {
			if err = client.Remove(current.path); err != nil {
				return sftpFault(err)
			}
			continue
		}
		pending = append(pending, step{current.path, true})
		entries, err := client.ReadDir(current.path)
		if err != nil {
			return sftpFault(err)
		}
		for _, entry := range entries {
			if !strings.ContainsAny(entry.Name(), "/\x00") {
				pending = append(pending, step{path: joinRemote(current.path, entry.Name())})
			}
		}
	}
	return nil
}

func copyRemote(client *sftp.Client, source, destination string) error {
	pending := [][2]string{{source, destination}}
	for len(pending) > 0 {
		source, destination = pending[len(pending)-1][0], pending[len(pending)-1][1]
		pending = pending[:len(pending)-1]
		metadata, err := client.Lstat(source)
		if err != nil {
			return sftpFault(err)
		}
		switch {
		case metadata.IsDir():
			if err = client.Mkdir(destination); err != nil {
				return sftpFault(err)
			}
			entries, err := client.ReadDir(source)
			if err != nil {
				return sftpFault(err)
			}
			for _, entry := range entries {
				if !strings.ContainsAny(entry.Name(), "/\x00") {
					pending = append(pending, [2]string{joinRemote(source, entry.Name()), joinRemote(destination, entry.Name())})
				}
			}
		case metadata.Mode()&os.ModeSymlink != 0:
			target, err := client.ReadLink(source)
			if err != nil {
				return sftpFault(err)
			}
			if err = client.Symlink(target, destination); err != nil {
				return sftpFault(err)
			}
		case metadata.Mode().IsRegular():
			if err = copyRemoteFile(client, source, destination); err != nil {
				return err
			}
		default:
			return invalid("Unsupported remote file type")
		}
	}
	return nil
}

func copyRemoteFile(client *sftp.Client, source, destination string) error {
	input, err := client.Open(source)
	if err != nil {
		return sftpFault(err)
	}
	defer input.Close()
	output, err := createExclusive(client, destination)
	if err != nil {
		return sftpFault(err)
	}
	if _, err = io.Copy(output, input); err != nil {
		_ = output.Close()
		return unknown("Remote copy interrupted; inspect the destination")
	}
	if err = output.Close(); err != nil {
		return sftpFault(err)
	}
	return nil
}

// boundedOutput retains the first outputLimit bytes and records any overflow.
type boundedOutput struct {
	data      []byte
	truncated bool
}

func (output *boundedOutput) Write(data []byte) (int, error) {
	count := min(len(data), outputLimit-len(output.data))
	output.data = append(output.data, data[:count]...)
	output.truncated = output.truncated || count < len(data)
	return len(data), nil
}

// Run executes one remote command and retains bounded output. Cancelling ctx
// closes the channel and returns ctx.Err(); the remote process may continue, so
// the caller reports that outcome as unknown.
func Run(ctx context.Context, client *cryptossh.Client, command string) (protocol.SshOutcome, error) {
	channel, requests, err := client.OpenChannel("session", nil)
	if err != nil {
		return protocol.SshOutcome{}, unavailable("SSH channel could not be opened")
	}
	defer channel.Close()
	stop := context.AfterFunc(ctx, func() { _ = channel.Close() })
	defer stop()
	accepted, err := channel.SendRequest("exec", true, cryptossh.Marshal(struct{ Command string }{command}))
	if err != nil {
		return protocol.SshOutcome{}, unknown("SSH command receipt is unavailable")
	}
	if !accepted {
		return protocol.SshOutcome{}, unavailable("SSH server rejected the command")
	}
	// A command that already finished has closed the channel; its exit status
	// below still decides the outcome.
	if err = channel.CloseWrite(); err != nil && !errors.Is(err, io.EOF) {
		return protocol.SshOutcome{}, unknown("SSH command connection was interrupted")
	}
	var stdout, stderr boundedOutput
	var exitCode *uint32
	var readers sync.WaitGroup
	readers.Add(2)
	go func() { defer readers.Done(); _, _ = io.Copy(&stdout, channel) }()
	go func() { defer readers.Done(); _, _ = io.Copy(&stderr, channel.Stderr()) }()
	for request := range requests {
		if request.Type == "exit-status" && len(request.Payload) == 4 {
			code := binary.BigEndian.Uint32(request.Payload)
			exitCode = &code
		}
		if request.WantReply {
			_ = request.Reply(false, nil)
		}
	}
	readers.Wait()
	if err = ctx.Err(); err != nil {
		return protocol.SshOutcome{}, err
	}
	if exitCode == nil {
		return protocol.SshOutcome{}, unknown("SSH command ended without an exit status")
	}
	return protocol.SshOutcome{Kind: "completed", Data: protocol.SshOutcomeCompletedPayload{
		ExitCode:  *exitCode,
		Stdout:    strings.ToValidUTF8(string(stdout.data), "\uFFFD"),
		Stderr:    strings.ToValidUTF8(string(stderr.data), "\uFFFD"),
		Truncated: stdout.truncated || stderr.truncated,
	}}, nil
}

func sftpFault(err error) *protocol.Fault {
	code := protocol.ErrorCodeUnavailable
	if errors.Is(err, os.ErrPermission) {
		code = protocol.ErrorCodePermissionDenied
	} else if errors.Is(err, os.ErrNotExist) {
		code = protocol.ErrorCodeNotFound
	}
	return &protocol.Fault{Code: code, Message: "SFTP operation failed: " + bounded(err.Error())}
}

func unknown(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: message}
}

// bounded strips control characters from untrusted text and keeps 256 runes.
func bounded(message string) string {
	var output strings.Builder
	count := 0
	for _, character := range message {
		if unicode.IsControl(character) {
			continue
		}
		if count == 256 {
			break
		}
		output.WriteRune(character)
		count++
	}
	return output.String()
}
