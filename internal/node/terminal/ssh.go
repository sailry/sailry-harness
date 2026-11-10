package terminal

import (
	"errors"
	"io"
	"sync"

	"github.com/sailry/sailry-harness/internal/protocol"
	cryptossh "golang.org/x/crypto/ssh"
)

// SSHConnection is the interactive shell of one SSH terminal. It owns its
// client: closing the terminal closes the session and the connection.
type SSHConnection struct {
	client  *cryptossh.Client
	session *cryptossh.Session
	input   io.WriteCloser
	output  *io.PipeReader
	mu      sync.Mutex
	exit    error
}

// OpenSSH requests a terminal and a shell on an authenticated connection.
// The caller keeps ownership of the client until the terminal starts.
func OpenSSH(client *cryptossh.Client, viewport protocol.TerminalViewport) (*SSHConnection, error) {
	session, err := client.NewSession()
	if err != nil {
		return nil, fault(protocol.ErrorCodeOutcomeUnknown, "SSH terminal channel could not be opened")
	}
	// Session.RequestPty cannot carry the pixel size, so the request is sent
	// in its wire form (RFC 4254 section 6.2) without terminal modes.
	accepted, err := session.SendRequest("pty-req", true, cryptossh.Marshal(struct {
		Term                         string
		Columns, Rows, Width, Height uint32
		Modes                        string
	}{"xterm-256color", uint32(viewport.Columns), uint32(viewport.Rows), viewport.PixelWidth, viewport.PixelHeight, "\x00"}))
	if err != nil {
		_ = session.Close()
		return nil, fault(protocol.ErrorCodeOutcomeUnknown, "SSH terminal request was interrupted")
	}
	if !accepted {
		_ = session.Close()
		return nil, fault(protocol.ErrorCodeUnavailable, "SSH server rejected the terminal request")
	}
	input, err := session.StdinPipe()
	if err != nil {
		_ = session.Close()
		return nil, err
	}
	output, writer := io.Pipe()
	session.Stdout, session.Stderr = writer, writer
	if err := session.Shell(); err != nil {
		_ = session.Close()
		return nil, fault(protocol.ErrorCodeOutcomeUnknown, "SSH shell request was interrupted")
	}
	connection := &SSHConnection{client: client, session: session, input: input, output: output}
	go func() {
		err := session.Wait()
		connection.mu.Lock()
		connection.exit = err
		connection.mu.Unlock()
		// The exit is recorded before the reader sees the end of output.
		_ = writer.Close()
	}()
	return connection, nil
}

func (connection *SSHConnection) Read(buffer []byte) (int, error) {
	return connection.output.Read(buffer)
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
