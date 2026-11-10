package link

import (
	"errors"
	"io"
	"net"
)

// Stream carries one authorized resource. CloseWrite finishes upload bytes while
// retaining the read side for the resource owner's publication acknowledgment.
type Stream interface {
	io.ReadWriteCloser
	CloseWrite() error
}

type localStream struct {
	reader net.Conn
	writer net.Conn
}

// StreamPair supplies the same independently closable read/write directions as
// a remote bidirectional stream. Standard pipes apply backpressure to each side.
func StreamPair() (Stream, Stream) {
	serverRead, clientWrite := net.Pipe()
	clientRead, serverWrite := net.Pipe()
	return &localStream{reader: clientRead, writer: clientWrite}, &localStream{reader: serverRead, writer: serverWrite}
}

func (s *localStream) Read(data []byte) (int, error)  { return s.reader.Read(data) }
func (s *localStream) Write(data []byte) (int, error) { return s.writer.Write(data) }
func (s *localStream) CloseWrite() error              { return s.writer.Close() }
func (s *localStream) Close() error                   { return errors.Join(s.reader.Close(), s.writer.Close()) }
