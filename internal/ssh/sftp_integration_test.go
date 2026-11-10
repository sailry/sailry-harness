//go:build integration

package ssh_test

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/pkg/sftp"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/ssh"
	cryptossh "golang.org/x/crypto/ssh"
	"lukechampine.com/blake3"
)

const fixturePassword = "isolated-ssh-password"

// sshFixture is an in-process SSH server on loopback with a generated host key.
// Its SFTP subsystem serves the real filesystem, so tests use absolute paths
// inside their own temporary directory. "exec" never starts a process.
type sshFixture struct {
	profile         protocol.SshProfile
	credential      protocol.SshCredential
	authentications atomic.Int32
	subsystems      atomic.Int32
}

func startSSHFixture(t *testing.T) *sshFixture {
	t.Helper()
	_, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	signer, err := cryptossh.NewSignerFromKey(private)
	if err != nil {
		t.Fatal(err)
	}
	fixture := &sshFixture{credential: protocol.SshCredential{Kind: "password", Data: protocol.SshCredentialPasswordPayload{Password: protocol.NewSecret(fixturePassword)}}}
	config := &cryptossh.ServerConfig{PasswordCallback: func(_ cryptossh.ConnMetadata, password []byte) (*cryptossh.Permissions, error) {
		fixture.authentications.Add(1)
		if string(password) != fixturePassword {
			return nil, errors.New("password rejected")
		}
		return nil, nil
	}}
	config.AddHostKey(signer)
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = listener.Close() })
	go func() {
		for {
			connection, err := listener.Accept()
			if err != nil {
				return
			}
			go fixture.serve(connection, config)
		}
	}()
	fixture.profile = protocol.SshProfile{
		ID: protocol.NewSshID(), Revision: 3, Name: "fixture", Host: "127.0.0.1", Port: uint16(listener.Addr().(*net.TCPAddr).Port),
		Username: "fixture", Authentication: protocol.SshAuthenticationPassword,
		HostKey: &protocol.SshHostKey{Algorithm: signer.PublicKey().Type(), Fingerprint: cryptossh.FingerprintSHA256(signer.PublicKey())},
	}
	return fixture
}

func (fixture *sshFixture) serve(connection net.Conn, config *cryptossh.ServerConfig) {
	defer connection.Close()
	server, channels, requests, err := cryptossh.NewServerConn(connection, config)
	if err != nil {
		return
	}
	defer server.Close()
	go cryptossh.DiscardRequests(requests)
	for offered := range channels {
		if offered.ChannelType() != "session" {
			_ = offered.Reject(cryptossh.UnknownChannelType, "session expected")
			continue
		}
		channel, requests, err := offered.Accept()
		if err != nil {
			return
		}
		go fixture.session(channel, requests)
	}
}

func (fixture *sshFixture) session(channel cryptossh.Channel, requests <-chan *cryptossh.Request) {
	defer channel.Close()
	for request := range requests {
		var payload struct{ Value string }
		_ = cryptossh.Unmarshal(request.Payload, &payload)
		switch {
		case request.Type == "subsystem" && payload.Value == "sftp":
			fixture.subsystems.Add(1)
			_ = request.Reply(true, nil)
			server, err := sftp.NewServer(channel)
			if err != nil {
				return
			}
			go func() { _ = server.Serve(); _ = channel.Close() }()
		case request.Type == "exec" && payload.Value == "rejected":
			_ = request.Reply(false, nil)
		case request.Type == "exec" && payload.Value == "hang":
			_ = request.Reply(true, nil)
		case request.Type == "exec" && payload.Value == "flood":
			_ = request.Reply(true, nil)
			_, _ = channel.Write(bytes.Repeat([]byte("x"), 200*1024))
			_, _ = channel.SendRequest("exit-status", false, cryptossh.Marshal(struct{ Status uint32 }{0}))
			return
		case request.Type == "exec":
			_ = request.Reply(true, nil)
			_, _ = io.WriteString(channel, "out:"+payload.Value)
			_, _ = io.WriteString(channel.Stderr(), "err:"+payload.Value)
			_, _ = channel.SendRequest("exit-status", false, cryptossh.Marshal(struct{ Status uint32 }{7}))
			return
		default:
			_ = request.Reply(false, nil)
		}
	}
}

func (fixture *sshFixture) connect(t *testing.T) *cryptossh.Client {
	t.Helper()
	connection, err := ssh.Connect(t.Context(), fixture.profile, fixture.credential)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = connection.Close() })
	return connection
}

func (fixture *sshFixture) sftp(t *testing.T) *sftp.Client {
	t.Helper()
	client, err := ssh.OpenSFTP(fixture.connect(t))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = client.Close() })
	return client
}

func canonicalTemp(t *testing.T) string {
	t.Helper()
	directory, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	return directory
}

func code(err error) protocol.ErrorCode {
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		return fault.Code
	}
	return ""
}

func digest(data []byte) string {
	sum := blake3.Sum256(data)
	return fmt.Sprintf("%x", sum[:])
}

// sendUpload plays the controller side of a staged upload stream.
func sendUpload(t *testing.T, transfers *files.Transfers, caller protocol.NodeID, stream protocol.StreamID, data []byte) {
	t.Helper()
	controller, err := transfers.Open(t.Context(), caller, stream)
	if err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	if _, err = controller.Write(data); err != nil {
		t.Fatal(err)
	}
	if err = controller.CloseWrite(); err != nil {
		t.Fatal(err)
	}
	ready, err := io.ReadAll(controller)
	if err != nil || len(ready) != 1 || ready[0] != protocol.FILE_UPLOAD_READY {
		t.Fatalf("upload acknowledgement = %v, %v", ready, err)
	}
}

func TestSftpIntegration(t *testing.T) {
	fixture := startSSHFixture(t)
	remote := canonicalTemp(t)
	data := make([]byte, 2*1024*1024)
	for index := range data {
		data[index] = byte(index % 251)
	}
	var caller protocol.NodeID
	caller[0] = 1

	t.Run("host key precedes authentication", func(t *testing.T) {
		for _, test := range []struct {
			name    string
			trusted *protocol.SshHostKey
			changed bool
		}{
			{"unknown", nil, false},
			{"changed", &protocol.SshHostKey{Algorithm: "ssh-ed25519", Fingerprint: "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}, true},
		} {
			t.Run(test.name, func(t *testing.T) {
				before := fixture.authentications.Load()
				profile := fixture.profile
				profile.HostKey = test.trusted
				_, err := ssh.Connect(t.Context(), profile, fixture.credential)
				var required *ssh.HostKeyRequired
				if !errors.As(err, &required) || required.Changed != test.changed || required.Key != *fixture.profile.HostKey {
					t.Fatalf("Connect = %v", err)
				}
				if fixture.authentications.Load() != before {
					t.Fatal("authentication ran before the host key was trusted")
				}
			})
		}
	})

	t.Run("browse", func(t *testing.T) {
		client := fixture.sftp(t)
		paged := filepath.Join(remote, "paged")
		if err := os.MkdirAll(filepath.Join(paged, "folder"), 0o755); err != nil {
			t.Fatal(err)
		}
		for index := range protocol.MAX_DIRECTORY_ENTRIES {
			if err := os.WriteFile(filepath.Join(paged, fmt.Sprintf("file-%04d", index)), nil, 0o600); err != nil {
				t.Fatal(err)
			}
		}
		first, err := ssh.List(client, paged+"/.", nil)
		if err != nil || first.Path != paged || len(first.Entries) != protocol.MAX_DIRECTORY_ENTRIES || first.Next == nil {
			t.Fatalf("first page = %q, %d entries, next %v, %v", first.Path, len(first.Entries), first.Next, err)
		}
		if first.Entries[0].Name != "folder" || first.Entries[0].Kind != protocol.EntryKindDirectory || first.Entries[1].Name != "file-0000" {
			t.Fatalf("order = %v", first.Entries[:2])
		}
		last, err := ssh.List(client, first.Path, first.Next)
		if err != nil || len(last.Entries) != 1 || last.Entries[0].Name != "file-0499" || last.Next != nil {
			t.Fatalf("last page = %v, %v", last, err)
		}
		if _, err = ssh.List(client, filepath.Join(remote, "absent"), nil); code(err) != protocol.ErrorCodeNotFound {
			t.Fatalf("missing directory = %v", err)
		}
	})

	t.Run("modify", func(t *testing.T) {
		client := fixture.sftp(t)
		base := filepath.Join(remote, "operations")
		create := func(directory bool) protocol.SshFileAction {
			return protocol.SshFileAction{Kind: "create", Data: &protocol.SshFileActionCreatePayload{Directory: directory}}
		}
		steps := []struct {
			name   string
			path   string
			action protocol.SshFileAction
			code   protocol.ErrorCode
		}{
			{"create directory", base, create(true), ""},
			{"create existing", base, create(true), protocol.ErrorCodeConflict},
			{"create nested", base + "/nested/", create(true), ""},
			{"create file", base + "/nested/empty", create(false), ""},
			{"copy tree", base, protocol.SshFileAction{Kind: "copy", Data: &protocol.SshFileActionCopyPayload{Destination: remote + "/copied"}}, ""},
			{"move tree", remote + "/copied", protocol.SshFileAction{Kind: "move", Data: &protocol.SshFileActionMovePayload{Destination: remote + "/moved ' 目录"}}, ""},
			{"copy into itself", base, protocol.SshFileAction{Kind: "copy", Data: &protocol.SshFileActionCopyPayload{Destination: base + "/nested/copy"}}, protocol.ErrorCodeInvalidRequest},
			{"move onto existing", base, protocol.SshFileAction{Kind: "move", Data: &protocol.SshFileActionMovePayload{Destination: remote + "/moved ' 目录"}}, protocol.ErrorCodeConflict},
			{"remove root", "/", protocol.SshFileAction{Kind: "remove"}, protocol.ErrorCodeInvalidRequest},
			{"remove tree", remote + "/moved ' 目录", protocol.SshFileAction{Kind: "remove"}, ""},
		}
		for _, step := range steps {
			if step.name == "copy tree" {
				if err := os.WriteFile(filepath.Join(base, "nested", "value"), []byte("preserved"), 0o600); err != nil {
					t.Fatal(err)
				}
				if err := os.Symlink(filepath.Join(remote, "paged"), filepath.Join(base, "link")); err != nil {
					t.Fatal(err)
				}
			}
			if err := ssh.Modify(client, step.path, step.action); code(err) != step.code {
				t.Fatalf("%s = %v, want code %q", step.name, err, step.code)
			}
			if step.name == "move tree" {
				value, err := os.ReadFile(filepath.Join(remote, "moved ' 目录", "nested", "value"))
				link, linkErr := os.Readlink(filepath.Join(remote, "moved ' 目录", "link"))
				if err != nil || string(value) != "preserved" || linkErr != nil || link != filepath.Join(remote, "paged") {
					t.Fatalf("copied tree = %q, %v, %q, %v", value, err, link, linkErr)
				}
			}
		}
		if _, err := os.Lstat(filepath.Join(remote, "moved ' 目录")); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("removed tree remains: %v", err)
		}
		if _, err := os.Stat(filepath.Join(remote, "paged", "folder")); err != nil {
			t.Fatalf("removing a link removed its target: %v", err)
		}
		if _, err := os.Stat(filepath.Join(base, "nested", "value")); err != nil {
			t.Fatal(err)
		}
	})

	t.Run("staged upload and download", func(t *testing.T) {
		client := fixture.sftp(t)
		transfers := files.NewTransfers(canonicalTemp(t))
		t.Cleanup(func() { _ = transfers.Close() })
		path := filepath.Join(remote, "文件 ' $(touch unexpected).bin")
		spec := protocol.SshUploadSpec{Profile: fixture.profile.ID, ExpectedRevision: fixture.profile.Revision, Path: path, Size: uint64(len(data)), Revision: strings.ToUpper(digest(data))}
		upload, err := transfers.StageSSHUpload(t.Context(), caller, spec)
		if err != nil || upload.Spec.Revision != digest(data) {
			t.Fatalf("stage = %+v, %v", upload, err)
		}
		if _, err = transfers.FinishSSHUpload(caller, client, fixture.profile, path, upload.Stream); code(err) != protocol.ErrorCodeConflict {
			t.Fatalf("finish before bytes = %v", err)
		}
		sendUpload(t, transfers, caller, upload.Stream, data)
		other, stale := caller, fixture.profile
		other[0]++
		stale.Revision++
		for _, test := range []struct {
			name    string
			caller  protocol.NodeID
			profile protocol.SshProfile
			path    string
		}{
			{"another caller", other, fixture.profile, path},
			{"another path", caller, fixture.profile, path + ".other"},
			{"another revision", caller, stale, path},
		} {
			if _, err = transfers.FinishSSHUpload(test.caller, client, test.profile, test.path, upload.Stream); code(err) != protocol.ErrorCodeNotFound {
				t.Fatalf("%s = %v", test.name, err)
			}
		}
		if _, err = os.Lstat(path); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("rejected finish published: %v", err)
		}
		size, err := transfers.FinishSSHUpload(caller, client, fixture.profile, path, upload.Stream)
		published, readErr := os.ReadFile(path)
		if err != nil || size != uint64(len(data)) || readErr != nil || !bytes.Equal(published, data) {
			t.Fatalf("finish = %d, %v, %v", size, err, readErr)
		}
		if _, err = os.Lstat(filepath.Join(remote, "unexpected")); !errors.Is(err, os.ErrNotExist) {
			t.Fatal("remote path was interpreted by a shell")
		}
		if _, err = transfers.FinishSSHUpload(caller, client, fixture.profile, path, upload.Stream); code(err) != protocol.ErrorCodeNotFound {
			t.Fatalf("second finish = %v", err)
		}

		if err = os.WriteFile(path, []byte("changed"), 0o600); err != nil {
			t.Fatal(err)
		}
		for _, overwrite := range []bool{false, true} {
			spec.Overwrite = overwrite
			upload, err = transfers.StageSSHUpload(t.Context(), caller, spec)
			if err != nil {
				t.Fatal(err)
			}
			sendUpload(t, transfers, caller, upload.Stream, data)
			_, err = transfers.FinishSSHUpload(caller, client, fixture.profile, path, upload.Stream)
			published, _ = os.ReadFile(path)
			if !overwrite && (code(err) != protocol.ErrorCodeConflict || string(published) != "changed") {
				t.Fatalf("conflicting finish = %v, %d bytes", err, len(published))
			}
			if overwrite && (err != nil || !bytes.Equal(published, data)) {
				t.Fatalf("overwriting finish = %v, %d bytes", err, len(published))
			}
			if _, err = transfers.FinishSSHUpload(caller, client, fixture.profile, path, upload.Stream); code(err) != protocol.ErrorCodeNotFound {
				t.Fatalf("consumed stage finished again: %v", err)
			}
		}

		download, err := transfers.DownloadSSH(t.Context(), caller, client, path)
		if err != nil || download.Path != path || download.Size != uint64(len(data)) || download.Revision != digest(data) {
			t.Fatalf("download = %+v, %v", download, err)
		}
		if _, err = transfers.Open(t.Context(), other, download.Stream); err == nil {
			t.Fatal("another caller opened the download")
		}
		stream, err := transfers.Open(t.Context(), caller, download.Stream)
		if err != nil {
			t.Fatal(err)
		}
		received, err := io.ReadAll(stream)
		_ = stream.Close()
		if err != nil || !bytes.Equal(received, data) {
			t.Fatalf("received %d bytes, %v", len(received), err)
		}
		if _, err = transfers.Open(t.Context(), caller, download.Stream); err == nil {
			t.Fatal("download stream opened twice")
		}
		if _, err = transfers.DownloadSSH(t.Context(), caller, client, remote); code(err) != protocol.ErrorCodeInvalidRequest {
			t.Fatalf("directory download = %v", err)
		}
	})

	t.Run("worktree transfer", func(t *testing.T) {
		storage := canonicalTemp(t)
		transfers := files.NewTransfers(storage)
		t.Cleanup(func() { _ = transfers.Close() })
		root := canonicalTemp(t)
		if err := os.WriteFile(filepath.Join(root, "source.bin"), data, 0o644); err != nil {
			t.Fatal(err)
		}
		if err := os.Mkdir(filepath.Join(remote, "inbox"), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.Symlink(remote, filepath.Join(root, "outside")); err != nil {
			t.Fatal(err)
		}
		transfer := func(direction protocol.SshDirection, path, remotePath string) (uint64, error) {
			return transfers.TransferSSH(t.Context(), fixture.connect(t), root, core.ManualFiles, protocol.SshTransfer{Path: path, RemotePath: remotePath, Direction: direction})
		}
		uploaded := filepath.Join(remote, "uploaded.bin")
		for _, destination := range []string{uploaded, filepath.Join(remote, "inbox") + "/"} {
			if size, err := transfer(protocol.SshDirectionUpload, "source.bin", destination); err != nil || size != uint64(len(data)) {
				t.Fatalf("upload to %s = %d, %v", destination, size, err)
			}
		}
		for _, path := range []string{uploaded, filepath.Join(remote, "inbox", "source.bin")} {
			published, err := os.ReadFile(path)
			metadata, statErr := os.Stat(path)
			if err != nil || statErr != nil || !bytes.Equal(published, data) {
				t.Fatalf("%s = %d bytes, %v, %v", path, len(published), err, statErr)
			}
			if runtime.GOOS != "windows" && metadata.Mode().Perm() != 0o600 {
				t.Fatalf("%s mode = %v", path, metadata.Mode().Perm())
			}
		}
		if err := os.Chmod(uploaded, 0o640); err != nil {
			t.Fatal(err)
		}
		if _, err := transfer(protocol.SshDirectionUpload, "source.bin", uploaded); err != nil {
			t.Fatal(err)
		}
		if metadata, err := os.Stat(uploaded); err != nil {
			t.Fatal(err)
		} else if runtime.GOOS != "windows" && metadata.Mode().Perm() != 0o640 {
			t.Fatalf("overwritten file mode = %v", metadata.Mode().Perm())
		}
		if size, err := transfer(protocol.SshDirectionDownload, "download.bin", uploaded); err != nil || size != uint64(len(data)) {
			t.Fatalf("download = %d, %v", size, err)
		}
		if published, err := os.ReadFile(filepath.Join(root, "download.bin")); err != nil || !bytes.Equal(published, data) {
			t.Fatalf("published %d bytes, %v", len(published), err)
		}
		for _, test := range []struct {
			name      string
			direction protocol.SshDirection
			path      string
			remote    string
			code      protocol.ErrorCode
			contains  string
		}{
			{"existing local file", protocol.SshDirectionDownload, "download.bin", uploaded, protocol.ErrorCodeRevisionConflict, "(local preparation)"},
			{"escape", protocol.SshDirectionDownload, "../escape", uploaded, protocol.ErrorCodeInvalidRequest, "(local preparation)"},
			{"git metadata", protocol.SshDirectionDownload, ".git/config", uploaded, protocol.ErrorCodePermissionDenied, "(local preparation)"},
			{"missing local source", protocol.SshDirectionUpload, "absent.bin", uploaded, protocol.ErrorCodeNotFound, "(local preparation)"},
			{"missing remote", protocol.SshDirectionDownload, "missing.bin", filepath.Join(remote, "absent"), protocol.ErrorCodeOutcomeUnknown, "remote open: SFTP status=NoSuchFile (2)"},
			{"missing remote parent", protocol.SshDirectionUpload, "source.bin", filepath.Join(remote, "missing-parent", "destination.bin"), protocol.ErrorCodeOutcomeUnknown, "remote open: SFTP status=NoSuchFile (2)"},
		} {
			t.Run(test.name, func(t *testing.T) {
				_, err := transfer(test.direction, test.path, test.remote)
				if code(err) != test.code || !strings.Contains(err.Error(), test.contains) {
					t.Fatalf("transfer = %v", err)
				}
			})
		}
		if _, err := transfer(protocol.SshDirectionDownload, "outside/escape", uploaded); err == nil {
			t.Fatal("download followed a link out of the worktree")
		}
		for _, absent := range []string{filepath.Join(root, "missing.bin"), filepath.Join(remote, "escape"), filepath.Join(remote, "missing-parent")} {
			if _, err := os.Lstat(absent); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("%s exists: %v", absent, err)
			}
		}
		leftovers, err := os.ReadDir(filepath.Join(storage, "tmp"))
		if err != nil || len(leftovers) != 0 {
			t.Fatalf("staging leftovers = %v, %v", leftovers, err)
		}

		inside := files.NewTransfers(root)
		t.Cleanup(func() { _ = inside.Close() })
		if _, err = inside.TransferSSH(t.Context(), fixture.connect(t), root, core.ManualFiles, protocol.SshTransfer{Path: "source.bin", RemotePath: uploaded, Direction: protocol.SshDirectionUpload}); code(err) != protocol.ErrorCodePermissionDenied {
			t.Fatalf("Node storage transfer = %v", err)
		}
	})

	t.Run("run", func(t *testing.T) {
		connection := fixture.connect(t)
		outcome, err := ssh.Run(t.Context(), connection, "echo fixture")
		completed, ok := outcome.Data.(protocol.SshOutcomeCompletedPayload)
		if err != nil || !ok || outcome.Kind != "completed" || completed.ExitCode != 7 || completed.Stdout != "out:echo fixture" || completed.Stderr != "err:echo fixture" || completed.Truncated {
			t.Fatalf("run = %+v, %v", outcome, err)
		}
		outcome, err = ssh.Run(t.Context(), connection, "flood")
		completed, _ = outcome.Data.(protocol.SshOutcomeCompletedPayload)
		if err != nil || len(completed.Stdout) != 128*1024 || !completed.Truncated || completed.ExitCode != 0 {
			t.Fatalf("flood = %d bytes, truncated %v, %v", len(completed.Stdout), completed.Truncated, err)
		}
		if _, err = ssh.Run(t.Context(), connection, "rejected"); code(err) != protocol.ErrorCodeUnavailable {
			t.Fatalf("rejected command = %v", err)
		}
		ctx, cancel := context.WithCancel(t.Context())
		time.AfterFunc(100*time.Millisecond, cancel)
		if _, err = ssh.Run(ctx, connection, "hang"); !errors.Is(err, context.Canceled) {
			t.Fatalf("cancelled command = %v", err)
		}
	})
}
