package terminal

import (
	"errors"
	"io"
	"sync"

	"github.com/sailry/sailry-harness/internal/protocol"
	cryptossh "golang.org/x/crypto/ssh"
)

// sshStartupLimit bounds the output kept while the server has not yet
// confirmed the terminal and shell requests.
const sshStartupLimit = 128 * 1024

// SSHConnection is the interactive shell of one SSH terminal. It owns its
// client: closing the terminal closes the session and the connection.
type SSHConnection struct {
	client  *cryptossh.Client
	session *cryptossh.Session
	input   io.WriteCloser
	output  *io.PipeReader
	writer  *io.PipeWriter
	// observe sees each piece of output as the terminal reads it.
	observe func([]byte)
	mu      sync.Mutex
	// While opening, output collects in startup, which is read first.
	opening  bool
	startup  []byte
	exceeded bool
	exit     error
}

// OpenSSH requests a terminal and a shell on an authenticated connection.
// The caller keeps ownership of the client until the terminal starts.
func OpenSSH(client *cryptossh.Client, viewport protocol.TerminalViewport) (*SSHConnection, error) {
	session, err := client.NewSession()
	if err != nil {
		return nil, fault(protocol.ErrorCodeOutcomeUnknown, "SSH terminal channel could not be opened")
	}
	var stdout, stderr io.Reader
	input, err := session.StdinPipe()
	if err == nil {
		stdout, err = session.StdoutPipe()
	}
	if err == nil {
		stderr, err = session.StderrPipe()
	}
	if err != nil {
		_ = session.Close()
		return nil, err
	}
	output, writer := io.Pipe()
	connection := &SSHConnection{client: client, session: session, input: input, output: output, writer: writer, opening: true}
	// Output is read from the start: what precedes the confirmations counts
	// against the startup limit.
	var pumps sync.WaitGroup
	for _, source := range []io.Reader{stdout, stderr} {
		pumps.Go(func() { connection.pump(source) })
	}
	// Session.RequestPty cannot carry the pixel size, so the request is sent
	// in its wire form (RFC 4254 section 6.2) without terminal modes.
	accepted, err := session.SendRequest("pty-req", true, cryptossh.Marshal(struct {
		Term                         string
		Columns, Rows, Width, Height uint32
		Modes                        string
	}{"xterm-256color", uint32(viewport.Columns), uint32(viewport.Rows), viewport.PixelWidth, viewport.PixelHeight, "\x00"}))
	if err != nil {
		return nil, connection.abandon(protocol.ErrorCodeOutcomeUnknown, "SSH terminal request was interrupted")
	}
	if !accepted {
		return nil, connection.abandon(protocol.ErrorCodeUnavailable, "SSH server rejected the terminal request")
	}
	if err := session.Shell(); err != nil {
		return nil, connection.abandon(protocol.ErrorCodeOutcomeUnknown, "SSH shell request was interrupted")
	}
	connection.mu.Lock()
	connection.opening = false
	exceeded := connection.exceeded
	connection.mu.Unlock()
	if exceeded {
		return nil, connection.abandon(protocol.ErrorCodeOutcomeUnknown, "SSH terminal startup output exceeds the limit")
	}
	go func() {
		pumps.Wait()
		err := session.Wait()
		connection.mu.Lock()
		connection.exit = err
		connection.mu.Unlock()
		// The exit is recorded before the reader sees the end of output.
		_ = writer.Close()
	}()
	return connection, nil
}

// abandon ends an opening that failed. Output beyond the startup limit
// closes the channel under a pending request, so it is the cause reported.
func (connection *SSHConnection) abandon(code protocol.ErrorCode, message string) error {
	_ = connection.session.Close()
	_ = connection.output.Close()
	connection.mu.Lock()
	defer connection.mu.Unlock()
	if connection.exceeded {
		return fault(protocol.ErrorCodeOutcomeUnknown, "SSH terminal startup output exceeds the limit")
	}
	return fault(code, message)
}

// pump moves one remote stream into the terminal's output until either ends.
func (connection *SSHConnection) pump(source io.Reader) {
	buffer := make([]byte, 32*1024)
	for {
		count, err := source.Read(buffer)
		if count > 0 && !connection.deliver(buffer[:count]) || err != nil {
			return
		}
	}
}

func (connection *SSHConnection) deliver(output []byte) bool {
	connection.mu.Lock()
	opening := connection.opening
	if opening && !connection.exceeded {
		if len(connection.startup)+len(output) > sshStartupLimit {
			connection.exceeded = true
		} else {
			connection.startup = append(connection.startup, output...)
		}
	}
	exceeded := connection.exceeded
	connection.mu.Unlock()
	if exceeded {
		// Closing the channel releases a request still awaiting its reply.
		_ = connection.session.Close()
		return false
	}
	if opening {
		return true
	}
	_, err := connection.writer.Write(output)
	return err == nil
}

func (connection *SSHConnection) Read(buffer []byte) (int, error) {
	connection.mu.Lock()
	count := copy(buffer, connection.startup)
	connection.startup = connection.startup[count:]
	connection.mu.Unlock()
	var err error
	if count == 0 {
		count, err = connection.output.Read(buffer)
	}
	if count > 0 && connection.observe != nil {
		connection.observe(buffer[:count])
	}
	return count, err
}

func (connection *SSHConnection) Write(body []byte) (int, error) {
	return connection.input.Write(body)
}

func (connection *SSHConnection) Close() error {
	_ = connection.output.Close()
	_ = connection.session.Close()
	return connection.client.Close()
}

// ResizePixels sends the window change with its pixel size.
func (connection *SSHConnection) ResizePixels(columns, rows, cellWidth, cellHeight int) error {
	_, err := connection.session.SendRequest("window-change", false, cryptossh.Marshal(struct {
		Columns, Rows, Width, Height uint32
	}{uint32(columns), uint32(rows), uint32(columns * cellWidth), uint32(rows * cellHeight)}))
	return err
}

// status reports how the remote shell ended.
func (connection *SSHConnection) status() protocol.TerminalStatus {
	connection.mu.Lock()
	err := connection.exit
	connection.mu.Unlock()
	var exit *cryptossh.ExitError
	switch {
	case err == nil:
		return protocol.TerminalStatus{Kind: "exited", Data: protocol.TerminalStatusExitedPayload{Code: 0}}
	case errors.As(err, &exit) && exit.Signal() != "":
		return protocol.TerminalStatus{Kind: "failed", Data: protocol.TerminalStatusFailedPayload{Message: "SSH shell exited after a signal"}}
	case errors.As(err, &exit):
		return protocol.TerminalStatus{Kind: "exited", Data: protocol.TerminalStatusExitedPayload{Code: uint32(exit.ExitStatus())}}
	}
	return protocol.TerminalStatus{Kind: "failed", Data: protocol.TerminalStatusFailedPayload{Message: "SSH terminal ended without an exit status"}}
}
