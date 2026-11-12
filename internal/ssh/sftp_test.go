package ssh

import (
	"context"
	"crypto/ecdsa"
	"crypto/ed25519"
	"crypto/elliptic"
	"crypto/rand"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"net"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"syscall"
	"testing"
	"testing/synctest"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/pkg/sftp"
	"github.com/sailry/sailry-harness/internal/protocol"
	cryptossh "golang.org/x/crypto/ssh"
)

type fakeFileInfo struct {
	size     int64
	modified time.Time
}

func (info fakeFileInfo) Name() string       { return "fixture" }
func (info fakeFileInfo) Size() int64        { return info.size }
func (info fakeFileInfo) Mode() fs.FileMode  { return 0o600 }
func (info fakeFileInfo) ModTime() time.Time { return info.modified }
func (info fakeFileInfo) IsDir() bool        { return false }
func (info fakeFileInfo) Sys() any           { return nil }

func faultCode(err error) protocol.ErrorCode {
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		return fault.Code
	}
	return ""
}

// replyCut loses the server's reply to an armed rename request.
type replyCut struct{ armed, lost atomic.Bool }

// renameWatch is the client's request stream.
type renameWatch struct {
	io.WriteCloser
	cut *replyCut
}

func (watch renameWatch) Write(data []byte) (int, error) {
	// An SFTP packet is a length, then its type; 18 is SSH_FXP_RENAME.
	if len(data) >= 5 && data[4] == 18 && watch.cut.armed.Load() {
		watch.cut.lost.Store(true)
	}
	return watch.WriteCloser.Write(data)
}

// lossyReplies is the server's reply stream.
type lossyReplies struct {
	*io.PipeWriter
	cut *replyCut
}

func (replies lossyReplies) Write(data []byte) (int, error) {
	if replies.cut.lost.Load() {
		_ = replies.PipeWriter.CloseWithError(io.ErrClosedPipe)
		return len(data), nil
	}
	return replies.PipeWriter.Write(data)
}

func dialError(code syscall.Errno) error {
	return &net.OpError{Op: "dial", Net: "tcp", Err: os.NewSyscallError("connect", code)}
}

func TestSftpPaths(t *testing.T) {
	t.Run("validation", func(t *testing.T) {
		for _, test := range []struct {
			name           string
			path           string
			browse, remote bool
		}{
			{"relative", "folder/file.bin", true, true},
			{"shell text stays literal", "文件 ' $(touch unexpected).bin", true, true},
			{"empty", "", false, false},
			{"nul", "a\x00b", false, false},
			{"newline is only a transfer fault", "a\nb", true, false},
			{"longest", strings.Repeat("a", 4096), true, true},
			{"too long", strings.Repeat("a", 4097), false, false},
		} {
			t.Run(test.name, func(t *testing.T) {
				if err := ValidatePath(test.path); (err == nil) != test.browse {
					t.Fatalf("ValidatePath = %v, want valid %v", err, test.browse)
				} else if err != nil && faultCode(err) != protocol.ErrorCodeInvalidRequest {
					t.Fatalf("ValidatePath code = %q", faultCode(err))
				}
				if err := ValidateRemotePath(test.path); (err == nil) != test.remote {
					t.Fatalf("ValidateRemotePath = %v, want valid %v", err, test.remote)
				}
			})
		}
	})

	t.Run("mutation targets", func(t *testing.T) {
		for _, test := range []struct {
			path, parent, name string
			valid              bool
		}{
			{"operations", ".", "operations", true},
			{"operations/nested", "operations", "nested", true},
			{"operations/nested/", "operations", "nested", true},
			{"/top", "/", "top", true},
			{"moved ' 目录", ".", "moved ' 目录", true},
			{"/", "", "", false},
			{"//", "", "", false},
			{".", "", "", false},
			{"..", "", "", false},
			{"folder/..", "", "", false},
			{"folder/.", "", "", false},
			{"", "", "", false},
		} {
			t.Run(test.path, func(t *testing.T) {
				parent, name, err := splitTarget(test.path)
				if (err == nil) != test.valid || parent != test.parent || name != test.name {
					t.Fatalf("splitTarget = %q, %q, %v", parent, name, err)
				}
				if err != nil && faultCode(err) != protocol.ErrorCodeInvalidRequest {
					t.Fatalf("code = %q", faultCode(err))
				}
			})
		}
		for _, test := range []struct{ parent, name, joined string }{
			{"/", "top", "/top"},
			{"/home/user", "file", "/home/user/file"},
			{"/home/user/", "file", "/home/user/file"},
		} {
			if joined := joinRemote(test.parent, test.name); joined != test.joined {
				t.Fatalf("joinRemote(%q, %q) = %q", test.parent, test.name, joined)
			}
		}
		for _, test := range []struct {
			source, destination string
			nested              bool
		}{
			{"/a/operations", "/a/operations", true},
			{"/a/operations", "/a/operations/nested/copy", true},
			{"/a/operations", "/a/operations-copy", false},
			{"/a/operations", "/a/copied", false},
		} {
			if nested := nests(test.source, test.destination); nested != test.nested {
				t.Fatalf("nests(%q, %q) = %v", test.source, test.destination, nested)
			}
		}
	})

	t.Run("entry kinds", func(t *testing.T) {
		for _, test := range []struct {
			mode os.FileMode
			kind protocol.EntryKind
		}{
			{os.ModeDir | 0o755, protocol.EntryKindDirectory},
			{0o644, protocol.EntryKindFile},
			{os.ModeSymlink | 0o777, protocol.EntryKindSymlink},
			{os.ModeSocket | 0o600, protocol.EntryKindOther},
			{os.ModeNamedPipe | 0o600, protocol.EntryKindOther},
		} {
			if kind := entryKind(test.mode); kind != test.kind {
				t.Fatalf("entryKind(%v) = %q, want %q", test.mode, kind, test.kind)
			}
		}
	})

	t.Run("ordering", func(t *testing.T) {
		entries := []protocol.FileEntry{
			{Name: "b.txt", Kind: protocol.EntryKindFile},
			{Name: "zeta", Kind: protocol.EntryKindDirectory},
			{Name: "文件", Kind: protocol.EntryKindFile},
			{Name: "B.txt", Kind: protocol.EntryKindFile},
			{Name: "alpha", Kind: protocol.EntryKindDirectory},
			{Name: "link", Kind: protocol.EntryKindSymlink},
		}
		page, next := PageEntries(entries, nil)
		var names []string
		for _, entry := range page {
			names = append(names, entry.Name)
		}
		if got, want := strings.Join(names, ","), "alpha,zeta,B.txt,b.txt,link,文件"; got != want || next != nil {
			t.Fatalf("order = %s, next = %v", got, next)
		}
		for _, test := range []struct {
			name  string
			after protocol.SshCursor
			want  string
		}{
			{"after a directory", protocol.SshCursor{Directory: true, Name: "alpha"}, "zeta,B.txt,b.txt,link,文件"},
			{"after the last directory", protocol.SshCursor{Directory: true, Name: "zeta"}, "B.txt,b.txt,link,文件"},
			{"after a file", protocol.SshCursor{Name: "b.txt"}, "link,文件"},
			{"after a removed file", protocol.SshCursor{Name: "c"}, "link,文件"},
			{"after everything", protocol.SshCursor{Name: "\U0010FFFF"}, ""},
		} {
			t.Run(test.name, func(t *testing.T) {
				page, next := PageEntries(append([]protocol.FileEntry(nil), entries...), &test.after)
				var names []string
				for _, entry := range page {
					names = append(names, entry.Name)
				}
				if got := strings.Join(names, ","); got != test.want || next != nil {
					t.Fatalf("page = %s, next = %v", got, next)
				}
			})
		}
	})

	t.Run("paging", func(t *testing.T) {
		var entries []protocol.FileEntry
		for index := protocol.MAX_DIRECTORY_ENTRIES; index >= 0; index-- {
			entries = append(entries, protocol.FileEntry{Name: fmt.Sprintf("file-%04d", index), Kind: protocol.EntryKindFile})
		}
		first, next := PageEntries(append([]protocol.FileEntry(nil), entries...), nil)
		if len(first) != protocol.MAX_DIRECTORY_ENTRIES || next == nil || next.Directory || next.Name != "file-0499" {
			t.Fatalf("first page = %d entries, next = %v", len(first), next)
		}
		last, end := PageEntries(append([]protocol.FileEntry(nil), entries...), next)
		if len(last) != 1 || last[0].Name != "file-0500" || end != nil {
			t.Fatalf("last page = %v, next = %v", last, end)
		}
		exact, end := PageEntries(append([]protocol.FileEntry(nil), entries[1:]...), nil)
		if len(exact) != protocol.MAX_DIRECTORY_ENTRIES || end != nil {
			t.Fatalf("exact page = %d entries, next = %v", len(exact), end)
		}
	})

	t.Run("connection recovery", func(t *testing.T) {
		unreachable, refused := dialError(syscall.EHOSTUNREACH), dialError(syscall.ECONNREFUSED)
		for _, test := range []struct {
			name     string
			goos     string
			failures []error
			calls    int
			want     error
		}{
			{"recovers after repeated failures", "darwin", []error{unreachable, unreachable, unreachable, unreachable, nil}, 5, nil},
			{"exhausts the bounded recovery", "darwin", []error{unreachable, unreachable, unreachable, unreachable, unreachable, nil}, 5, unreachable},
			{"keeps the last connection error", "darwin", []error{unreachable, refused, nil}, 2, refused},
			{"returns other failures at once", "darwin", []error{refused, nil}, 1, refused},
			{"leaves other platforms unchanged", "linux", []error{unreachable, nil}, 1, unreachable},
			{"succeeds without waiting", "darwin", []error{nil}, 1, nil},
		} {
			t.Run(test.name, func(t *testing.T) {
				ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
				defer cancel()
				clock := clockwork.NewFakeClock()
				started := clock.Now()
				var attempts []time.Duration
				done := make(chan error, 1)
				go func() {
					done <- recoverUnreachable(ctx, clock, test.goos, func() error {
						attempts = append(attempts, clock.Since(started))
						return test.failures[len(attempts)-1]
					})
				}()
				for retry := 0; retry < test.calls-1; retry++ {
					if err := clock.BlockUntilContext(ctx, 1); err != nil {
						t.Fatalf("retry %d was not scheduled: %v", retry, err)
					}
					clock.Advance(retryDelays[retry])
				}
				if err := <-done; err != test.want {
					t.Fatalf("error = %v, want %v", err, test.want)
				}
				if len(attempts) != test.calls {
					t.Fatalf("attempts = %d, want %d", len(attempts), test.calls)
				}
				var elapsed time.Duration
				for index, at := range attempts {
					if at != elapsed {
						t.Fatalf("attempt %d ran at %v, want %v", index, at, elapsed)
					}
					if index < len(retryDelays) {
						elapsed += retryDelays[index]
					}
				}
			})
		}
		t.Run("cancellation drops the pending retry", func(t *testing.T) {
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			clock := clockwork.NewFakeClock()
			calls := 0
			done := make(chan error, 1)
			go func() {
				done <- recoverUnreachable(ctx, clock, "darwin", func() error { calls++; return unreachable })
			}()
			guard, release := context.WithTimeout(t.Context(), 10*time.Second)
			defer release()
			if err := clock.BlockUntilContext(guard, 1); err != nil {
				t.Fatal(err)
			}
			cancel()
			if err := <-done; err != unreachable || calls != 1 {
				t.Fatalf("error = %v after %d calls", err, calls)
			}
		})
		if got := []time.Duration{250 * time.Millisecond, 500 * time.Millisecond, time.Second, 2 * time.Second}; retryDelays != [4]time.Duration(got) {
			t.Fatalf("retry delays = %v", retryDelays)
		}
		for _, test := range []struct {
			err  error
			dial bool
		}{
			{unavailable("SSH connection could not be established"), true},
			{unavailable("SSH authentication or handshake failed"), false},
			{unavailable("SSH connection was interrupted"), false},
			{&HostKeyRequired{}, false},
		} {
			if dialFailed(test.err) != test.dial {
				t.Fatalf("dialFailed(%v) = %v", test.err, !test.dial)
			}
		}
	})
}

// memoryConn is one end of a buffered in-memory connection. net.Pipe has no
// buffer, and both sides of an SSH handshake write before they read.
type memoryConn struct {
	net.Conn
	incoming, outgoing chan []byte
	rest               []byte
	closed, peer       chan struct{}
	once               *sync.Once
}

func memoryPipe() (*memoryConn, *memoryConn) {
	first, second := make(chan []byte, 4096), make(chan []byte, 4096)
	near, far := make(chan struct{}), make(chan struct{})
	return &memoryConn{incoming: first, outgoing: second, closed: near, peer: far, once: new(sync.Once)},
		&memoryConn{incoming: second, outgoing: first, closed: far, peer: near, once: new(sync.Once)}
}

func (conn *memoryConn) Read(data []byte) (int, error) {
	if len(conn.rest) == 0 {
		select {
		case conn.rest = <-conn.incoming:
		default:
			select {
			case conn.rest = <-conn.incoming:
			case <-conn.closed:
				return 0, net.ErrClosed
			case <-conn.peer:
				return 0, io.EOF
			}
		}
	}
	count := copy(data, conn.rest)
	conn.rest = conn.rest[count:]
	return count, nil
}

func (conn *memoryConn) Write(data []byte) (int, error) {
	select {
	case <-conn.closed:
		return 0, net.ErrClosed
	case <-conn.peer:
		return 0, io.ErrClosedPipe
	case conn.outgoing <- append([]byte(nil), data...):
		return len(data), nil
	}
}

func (conn *memoryConn) Close() error {
	conn.once.Do(func() { close(conn.closed) })
	return nil
}

func (*memoryConn) LocalAddr() net.Addr  { return &net.UnixAddr{Name: "fixture"} }
func (*memoryConn) RemoteAddr() net.Addr { return &net.UnixAddr{Name: "fixture"} }

// memoryServer answers one SSH handshake over an in-memory connection with
// the given host keys and returns the client's end; served receives the
// server's global requests once the handshake is done.
func memoryServer(served func(<-chan *cryptossh.Request), keys ...cryptossh.Signer) net.Conn {
	config := &cryptossh.ServerConfig{NoClientAuth: true}
	for _, key := range keys {
		config.AddHostKey(key)
	}
	near, far := memoryPipe()
	go func() {
		_, channels, requests, err := cryptossh.NewServerConn(far, config)
		if err != nil {
			_ = far.Close()
			return
		}
		go func() {
			for channel := range channels {
				_ = channel.Reject(cryptossh.Prohibited, "fixture")
			}
		}()
		served(requests)
	}()
	return near
}

func TestConnection(t *testing.T) {
	_, edwards, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	curve, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	first, err := cryptossh.NewSignerFromKey(edwards)
	if err != nil {
		t.Fatal(err)
	}
	second, err := cryptossh.NewSignerFromKey(curve)
	if err != nil {
		t.Fatal(err)
	}
	t.Run("the trusted host key's algorithm is preferred", func(t *testing.T) {
		// The server holds two keys; whichever the library would choose by
		// itself, the one that was trusted is the one presented.
		for _, trusted := range []cryptossh.Signer{first, second} {
			t.Run(trusted.PublicKey().Type(), func(t *testing.T) {
				near := memoryServer(cryptossh.DiscardRequests, first, second)
				defer near.Close()
				profile := protocol.SshProfile{Username: "fixture", HostKey: &protocol.SshHostKey{
					Algorithm: trusted.PublicKey().Type(), Fingerprint: cryptossh.FingerprintSHA256(trusted.PublicKey()),
				}}
				connection, _, _, err := cryptossh.NewClientConn(near, "fixture", clientConfig(profile, cryptossh.Password("")))
				if err != nil {
					t.Fatalf("a server with a second key type read as changed: %v", err)
				}
				_ = connection.Close()
			})
		}
		if hostKeyAlgorithms(nil) != nil {
			t.Fatal("an unknown host restricted the library's order")
		}
		// An RSA key signs with SHA-2; the library does not offer SHA-1.
		if got := hostKeyAlgorithms(&protocol.SshHostKey{Algorithm: cryptossh.KeyAlgoRSA}); len(got) < 3 ||
			got[0] != cryptossh.KeyAlgoRSASHA512 || got[1] != cryptossh.KeyAlgoRSASHA256 || slices.Contains(got, cryptossh.KeyAlgoRSA) {
			t.Fatalf("a trusted RSA key prefers %v", got)
		}
		if got := hostKeyAlgorithms(&protocol.SshHostKey{Algorithm: "fixture-unsupported"}); got != nil {
			t.Fatalf("an unsupported trusted algorithm prefers %v", got)
		}
	})
	t.Run("keepalive", func(t *testing.T) {
		trust := &protocol.SshHostKey{Algorithm: first.PublicKey().Type(), Fingerprint: cryptossh.FingerprintSHA256(first.PublicKey())}
		for _, test := range []struct {
			name    string
			answers bool
		}{
			{"a server that answers stays connected", true},
			{"a server that stopped answering is closed", false},
		} {
			t.Run(test.name, func(t *testing.T) {
				synctest.Test(t, func(t *testing.T) {
					asked := make(chan string, 64)
					release := make(chan struct{})
					near := memoryServer(func(requests <-chan *cryptossh.Request) {
						for request := range requests {
							asked <- request.Type
							if !test.answers {
								// The request stays unanswered until the test ends.
								<-release
							}
							if request.WantReply {
								_ = request.Reply(false, nil)
							}
						}
					}, first)
					defer near.Close()
					connection, channels, requests, err := cryptossh.NewClientConn(near, "fixture", clientConfig(protocol.SshProfile{Username: "fixture", HostKey: trust}, cryptossh.Password("")))
					if err != nil {
						t.Fatal(err)
					}
					client := cryptossh.NewClient(connection, channels, requests)
					ended := make(chan struct{})
					go func() { _ = client.Wait(); close(ended) }()
					started := time.Now()
					go keepAlive(client, keepaliveInterval, keepaliveMisses)
					if test.answers {
						time.Sleep(10*keepaliveInterval + time.Second)
						synctest.Wait()
						select {
						case <-ended:
							t.Fatal("an answering server was disconnected")
						default:
						}
						if len(asked) != 10 {
							t.Fatalf("%d keepalives in ten intervals", len(asked))
						}
						_ = client.Close()
					}
					<-ended
					if !test.answers {
						if elapsed, want := time.Since(started), (1+keepaliveMisses)*keepaliveInterval; elapsed != want || len(asked) != 1 {
							t.Fatalf("closed after %v and %d keepalives, want %v", elapsed, len(asked), want)
						}
					}
					if kind := <-asked; kind != "keepalive@openssh.com" {
						t.Fatalf("keepalive request = %q", kind)
					}
					close(release)
				})
			})
		}
		if keepaliveInterval != 15*time.Second || keepaliveMisses != 3 {
			t.Fatalf("keepalive = %v x %d", keepaliveInterval, keepaliveMisses)
		}
	})
}

func TestTransferResume(t *testing.T) {
	t.Run("overwrite conflict", func(t *testing.T) {
		for _, test := range []struct {
			overwrite        bool
			exclusive, clear bool
		}{
			{false, true, false},
			{true, false, true},
		} {
			flags := uploadFlags(test.overwrite)
			if flags&os.O_WRONLY == 0 || flags&os.O_CREATE == 0 || (flags&os.O_EXCL != 0) != test.exclusive || (flags&os.O_TRUNC != 0) != test.clear {
				t.Fatalf("uploadFlags(%v) = %#x", test.overwrite, flags)
			}
		}
	})

	t.Run("destination", func(t *testing.T) {
		for _, test := range []struct {
			remote, local string
			directory     bool
			want          string
		}{
			{"destination.bin", "source.bin", false, "destination.bin"},
			{"folder/", "source.bin", true, "folder/source.bin"},
			{"folder", "nested/source.bin", true, "folder/source.bin"},
			{"/", "source.bin", true, "/source.bin"},
			{"folder/", "source.bin", false, "folder/"},
		} {
			if got := uploadDestination(test.remote, test.local, test.directory); got != test.want {
				t.Fatalf("uploadDestination(%q, %q, %v) = %q", test.remote, test.local, test.directory, got)
			}
		}
	})

	t.Run("metadata recheck", func(t *testing.T) {
		modified := time.Unix(1_700_000_000, 0)
		for _, test := range []struct {
			name          string
			before, after fakeFileInfo
			copied        uint64
			unchanged     bool
		}{
			{"unchanged", fakeFileInfo{7, modified}, fakeFileInfo{7, modified}, 7, true},
			{"empty", fakeFileInfo{0, modified}, fakeFileInfo{0, modified}, 0, true},
			{"grew", fakeFileInfo{7, modified}, fakeFileInfo{8, modified}, 7, false},
			{"rewritten in place", fakeFileInfo{7, modified}, fakeFileInfo{7, modified.Add(time.Second)}, 7, false},
			{"short copy", fakeFileInfo{7, modified}, fakeFileInfo{7, modified}, 6, false},
			{"long copy", fakeFileInfo{7, modified}, fakeFileInfo{7, modified}, 8, false},
		} {
			t.Run(test.name, func(t *testing.T) {
				if got := sourceUnchanged(test.before, test.after, test.copied); got != test.unchanged {
					t.Fatalf("sourceUnchanged = %v", got)
				}
			})
		}
	})

	t.Run("uncertain stages", func(t *testing.T) {
		noisy := "\x00" + strings.Repeat("界", 512) + "\r\n"
		for _, test := range []struct {
			name     string
			err      error
			code     protocol.ErrorCode
			contains string
		}{
			{"interrupted", Interrupted("outer timeout"), protocol.ErrorCodeOutcomeUnknown, "inspect the destination before retrying (outer timeout)"},
			{"missing remote", remoteFailure(stageOpen, os.ErrNotExist), protocol.ErrorCodeOutcomeUnknown, "(remote open: SFTP status=NoSuchFile (2))"},
			{"denied remote", remoteFailure(stageStat, os.ErrPermission), protocol.ErrorCodeOutcomeUnknown, "(remote stat: SFTP status=PermissionDenied (3))"},
			{"other remote", remoteFailure(stageOpen, errors.New("disconnected")), protocol.ErrorCodeOutcomeUnknown, "(remote open: SFTP: disconnected)"},
			{"remote close", ioFailure(stageClose, errors.New("close denied")), protocol.ErrorCodeOutcomeUnknown, "(remote close: I/O: close denied)"},
			{"local keeps its code", localFailure("local metadata", fs.ErrPermission), protocol.ErrorCodePermissionDenied, "(local metadata: I/O)"},
			{"local missing", localFailure("local write", fs.ErrNotExist), protocol.ErrorCodeNotFound, "(local write: I/O)"},
			{"plain error", AtStage(errors.New("temporary directory failed"), "local preparation"), protocol.ErrorCodeUnavailable, "temporary directory failed (local preparation)"},
			{"noisy remote", remoteFailure(stageOpen, errors.New(noisy)), protocol.ErrorCodeOutcomeUnknown, "界"},
			{"noisy io", ioFailure(stageClose, errors.New(noisy)), protocol.ErrorCodeOutcomeUnknown, "界"},
			{"noisy local", localFailure("local read", errors.New(noisy)), protocol.ErrorCodeInvalidRequest, "界"},
			{"change lost in transit", sftpChange(sftp.ErrSSHFxConnectionLost), protocol.ErrorCodeOutcomeUnknown, "SFTP reply was lost; inspect the remote entry before retrying"},
			{"change cut short", sftpChange(io.ErrUnexpectedEOF), protocol.ErrorCodeOutcomeUnknown, "SFTP reply was lost"},
			{"change refused", sftpChange(&sftp.StatusError{Code: 4}), protocol.ErrorCodeUnavailable, "SFTP operation failed"},
			{"change denied", sftpChange(os.ErrPermission), protocol.ErrorCodePermissionDenied, "SFTP operation failed"},
			{"change target missing", sftpChange(os.ErrNotExist), protocol.ErrorCodeNotFound, "SFTP operation failed"},
			{"read lost in transit", sftpRead(sftp.ErrSSHFxConnectionLost), protocol.ErrorCodeUnavailable, "SFTP operation failed"},
		} {
			t.Run(test.name, func(t *testing.T) {
				var fault *protocol.Fault
				if !errors.As(test.err, &fault) || fault.Code != test.code || !strings.Contains(fault.Message, test.contains) {
					t.Fatalf("fault = %v", test.err)
				}
				if strings.ContainsFunc(fault.Message, func(character rune) bool { return character < ' ' || character == 0x7f }) {
					t.Fatalf("message keeps control characters: %q", fault.Message)
				}
				if count := strings.Count(fault.Message, "界"); count > 256 {
					t.Fatalf("message keeps %d untrusted runes", count)
				}
			})
		}
		original := &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "file changed outside this operation"}
		staged := AtStage(original, "local preparation")
		if faultCode(staged) != protocol.ErrorCodeRevisionConflict || original.Message != "file changed outside this operation" {
			t.Fatalf("AtStage changed its input: %v, %v", staged, original)
		}
	})

	// A real server applies a move whose reply never arrives: the caller must
	// learn the outcome is unknown, while a refusal the server sends stays
	// definite.
	t.Run("lost mutation reply", func(t *testing.T) {
		if runtime.GOOS == "windows" {
			t.Skip("the in-process SFTP server needs POSIX paths")
		}
		root, err := filepath.EvalSymlinks(t.TempDir())
		if err != nil {
			t.Fatal(err)
		}
		requestsIn, requestsOut := io.Pipe()
		repliesIn, repliesOut := io.Pipe()
		cut := &replyCut{}
		server, err := sftp.NewServer(struct {
			io.Reader
			io.WriteCloser
		}{requestsIn, lossyReplies{repliesOut, cut}})
		if err != nil {
			t.Fatal(err)
		}
		go func() { _ = server.Serve() }()
		client, err := sftp.NewClientPipe(repliesIn, renameWatch{requestsOut, cut})
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = client.Close(); _ = server.Close() })
		source, destination := filepath.Join(root, "source.txt"), filepath.Join(root, "moved.txt")
		if err = os.WriteFile(source, []byte("payload"), 0o600); err != nil {
			t.Fatal(err)
		}
		move := func(from, to string) error {
			return Modify(client, from, protocol.SshFileAction{Kind: "move", Data: &protocol.SshFileActionMovePayload{Destination: to}})
		}
		if code := faultCode(move(filepath.Join(root, "absent.txt"), filepath.Join(root, "elsewhere.txt"))); code != protocol.ErrorCodeNotFound {
			t.Fatalf("refused move = %s", code)
		}
		cut.armed.Store(true)
		err = move(source, destination)
		if _, statErr := os.Stat(destination); statErr != nil {
			t.Fatalf("server did not apply the move: %v", statErr)
		}
		if faultCode(err) != protocol.ErrorCodeOutcomeUnknown {
			t.Fatalf("applied move with a lost reply = %v", err)
		}
	})

	t.Run("bounded command output", func(t *testing.T) {
		for _, test := range []struct {
			name      string
			writes    []int
			retained  int
			truncated bool
		}{
			{"below the limit", []int{10, 20}, 30, false},
			{"exactly the limit", []int{outputLimit}, outputLimit, false},
			{"one byte over", []int{outputLimit, 1}, outputLimit, true},
			{"split across the limit", []int{outputLimit - 1, 5, 5}, outputLimit, true},
		} {
			t.Run(test.name, func(t *testing.T) {
				var output boundedOutput
				for _, size := range test.writes {
					if count, err := output.Write(make([]byte, size)); count != size || err != nil {
						t.Fatalf("Write = %d, %v", count, err)
					}
				}
				if len(output.data) != test.retained || output.truncated != test.truncated {
					t.Fatalf("retained %d bytes, truncated %v", len(output.data), output.truncated)
				}
			})
		}
	})
}
