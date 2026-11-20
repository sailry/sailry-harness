package peertransport

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"

	"github.com/sailry/sailry-harness/internal/protocol"
)

var ErrClosed = errors.New("peer transport is closed")

type Address struct {
	ID    protocol.NodeID
	Addrs []json.RawMessage
}

func (address Address) MarshalJSON() ([]byte, error) {
	hints := address.Addrs
	if hints == nil {
		hints = []json.RawMessage{}
	}
	return json.Marshal(struct {
		ID    string            `json:"id"`
		Addrs []json.RawMessage `json:"addrs"`
	}{ID: address.ID.String(), Addrs: hints})
}

func (address *Address) UnmarshalJSON(data []byte) error {
	var encoded struct {
		ID    string            `json:"id"`
		Addrs []json.RawMessage `json:"addrs"`
	}
	if err := json.Unmarshal(data, &encoded); err != nil {
		return err
	}
	identity, err := ParseAddressID(encoded.ID)
	if err != nil {
		return err
	}
	*address = Address{ID: identity, Addrs: encoded.Addrs}
	return nil
}

func ParseAddressID(value string) (protocol.NodeID, error) {
	var identity protocol.NodeID
	if len(value) != hex.EncodedLen(len(identity)) {
		return identity, errors.New("endpoint identity must contain 32 bytes")
	}
	_, err := hex.Decode(identity[:], []byte(value))
	return identity, err
}

type ConnectionInfo struct {
	RemoteID protocol.NodeID
	ALPN     string
	Route    string
	RTTMS    *uint64
	Closed   *CloseReason
}

type CloseReason struct {
	Kind   string `json:"kind"`
	Code   uint64 `json:"code"`
	Reason string `json:"reason"`
}

type Stream interface {
	io.ReadWriteCloser
	CloseWrite() error
	ReadContext(context.Context, []byte) (int, error)
	WriteContext(context.Context, []byte) (int, error)
	WaitStopped(context.Context) (*uint64, error)
}

// Connection values are comparable. Incoming streams share one logical
// connection's Link compression state across physical path changes.
type Connection interface {
	Info(context.Context) (ConnectionInfo, error)
	WaitPath(context.Context) error
	OpenBi(context.Context) (Stream, error)
	AcceptBi(context.Context) (Stream, error)
	Close(uint64, string) error
}

type Endpoint interface {
	Address(context.Context) (Address, error)
	Online(context.Context) error
	NetworkChange(context.Context) error
	// SetRelays commits verified reservations or leaves the old selection
	// usable on failure. An empty list switches to direct operation.
	SetRelays(context.Context, []string) error
	Connect(context.Context, Address, string) (Connection, error)
	Accept(context.Context) (Connection, error)
	Close() error
}
