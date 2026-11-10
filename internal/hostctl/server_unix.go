//go:build !windows

package hostctl

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"net"
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type Server struct {
	listener  *net.UnixListener
	path      string
	identity  os.FileInfo
	endpoint  *link.Link
	transport link.Transport
	close     sync.Once
	err       error
}

func Listen(profile string, endpoint *link.Link, transport link.Transport) (*Server, error) {
	path := filepath.Join(profile, "control.sock")
	if metadata, err := os.Lstat(path); err == nil {
		if metadata.Mode()&os.ModeSocket == 0 {
			return nil, errors.New("Host control path is not a socket")
		}
		if err := os.Remove(path); err != nil {
			return nil, err
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return nil, err
	}
	listener, err := net.ListenUnix("unix", &net.UnixAddr{Name: path, Net: "unix"})
	if err != nil {
		return nil, err
	}
	listener.SetUnlinkOnClose(false)
	if err := os.Chmod(path, 0o600); err != nil {
		_ = listener.Close()
		_ = os.Remove(path)
		return nil, err
	}
	identity, err := os.Lstat(path)
	if err != nil {
		_ = listener.Close()
		return nil, err
	}
	return &Server{listener: listener, path: path, identity: identity, endpoint: endpoint, transport: transport}, nil
}

func (server *Server) Close() error {
	server.close.Do(func() {
		server.err = server.listener.Close()
		if metadata, err := os.Lstat(server.path); err == nil && os.SameFile(metadata, server.identity) {
			server.err = errors.Join(server.err, os.Remove(server.path))
		} else if err != nil && !errors.Is(err, os.ErrNotExist) {
			server.err = errors.Join(server.err, err)
		}
	})
	return server.err
}

func (server *Server) Serve(ctx context.Context) error {
	scoped, cancel := context.WithCancel(ctx)
	stop := context.AfterFunc(scoped, func() { _ = server.Close() })
	defer stop()
	var owners sync.WaitGroup
	defer func() {
		cancel()
		_ = server.Close()
		owners.Wait()
	}()
	for {
		connection, err := server.listener.AcceptUnix()
		if err != nil {
			if scoped.Err() != nil || errors.Is(err, net.ErrClosed) {
				return nil
			}
			return err
		}
		owners.Add(1)
		go func() {
			defer owners.Done()
			defer connection.Close()
			_ = server.handle(scoped, connection)
		}()
	}
}

func (server *Server) handle(ctx context.Context, connection *net.UnixConn) error {
	scoped, cancel := context.WithCancel(ctx)
	defer cancel()
	stop := context.AfterFunc(scoped, func() { _ = connection.Close() })
	defer stop()
	if err := connection.SetReadDeadline(time.Now().Add(10 * time.Second)); err != nil {
		return err
	}
	reader := bufio.NewReader(connection)
	line, err := readLine(reader, protocol.MAX_FRAME_BYTES)
	if err != nil {
		return err
	}
	if err := connection.SetReadDeadline(time.Time{}); err != nil {
		return err
	}
	disconnectedDone := make(chan struct{})
	go func() {
		defer close(disconnectedDone)
		var disconnected [1]byte
		_, _ = reader.Read(disconnected[:])
		cancel()
	}()
	defer func() {
		cancel()
		_ = connection.Close()
		<-disconnectedDone
	}()
	var frame struct {
		Command string `json:"command"`
	}
	if json.Unmarshal(line, &frame) != nil {
		return errors.New("invalid Host control request")
	}
	switch frame.Command {
	case "node_hello", "node_request", "node_subscribe":
		return serveTransport(scoped, line, connection, server.transport)
	case "submit", "wait":
		if len(line) > MaxTaskRequest {
			return errors.New("Host task request exceeds its limit")
		}
		return serveTask(scoped, line, connection, server.transport)
	case "share":
		return serveShare(scoped, line, connection, server.endpoint)
	}
	return errors.New("unsupported Host control request")
}

func connectControl(ctx context.Context, profile string) (net.Conn, error) {
	return (&net.Dialer{}).DialContext(ctx, "unix", filepath.Join(profile, "control.sock"))
}
