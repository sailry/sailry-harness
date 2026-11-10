//go:build windows

package hostctl

import (
	"context"
	"errors"
	"net"

	"github.com/sailry/sailry-harness/internal/link"
)

type Server struct{}

func Listen(string, *link.Link, link.Transport) (*Server, error) {
	return nil, errors.New("Host control requires a Unix socket")
}

func (*Server) Close() error { return nil }

func (*Server) Serve(context.Context) error {
	return errors.New("Host control requires a Unix socket")
}

func connectControl(context.Context, string) (net.Conn, error) {
	return nil, errors.New("task management requires a Unix Host control socket")
}
