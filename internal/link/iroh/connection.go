package iroh

import (
	"context"
	"runtime"
	"sync"
	"sync/atomic"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type Connection struct {
	handle    uint64
	closed    atomic.Bool
	closeOnce sync.Once
	closeErr  error
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

func (connection *Connection) Info(ctx context.Context) (ConnectionInfo, error) {
	if connection.closed.Load() {
		return ConnectionInfo{}, ErrClosed
	}
	var result struct {
		RemoteID string       `json:"remote_id"`
		ALPN     string       `json:"alpn"`
		Route    string       `json:"route"`
		RTTMS    *uint64      `json:"rtt_ms"`
		Closed   *CloseReason `json:"closed"`
	}
	err := invokeJSON(ctx, func(functions *abi) uintptr {
		return functions.connectionInfo(connection.handle)
	}, &result)
	if err != nil {
		return ConnectionInfo{}, err
	}
	identity, err := decodeIdentity(result.RemoteID)
	if err != nil {
		return ConnectionInfo{}, err
	}
	return ConnectionInfo{
		RemoteID: identity,
		ALPN:     result.ALPN,
		Route:    result.Route,
		RTTMS:    result.RTTMS,
		Closed:   result.Closed,
	}, nil
}

func (connection *Connection) WaitPath(ctx context.Context) error {
	if connection.closed.Load() {
		return ErrClosed
	}
	return invokeJSON(ctx, func(functions *abi) uintptr {
		return functions.connectionWaitPath(connection.handle)
	}, nil)
}

func (connection *Connection) OpenBi(ctx context.Context) (*BiStream, error) {
	if connection.closed.Load() {
		return nil, ErrClosed
	}
	var result streamHandles
	err := invokeJSON(ctx, func(functions *abi) uintptr {
		return functions.openBi(connection.handle)
	}, &result)
	if err != nil {
		return nil, err
	}
	return &BiStream{handles: result}, nil
}

func (connection *Connection) AcceptBi(ctx context.Context) (*BiStream, error) {
	if connection.closed.Load() {
		return nil, ErrClosed
	}
	var result streamHandles
	err := invokeJSON(ctx, func(functions *abi) uintptr {
		return functions.acceptBi(connection.handle)
	}, &result)
	if err != nil {
		return nil, err
	}
	return &BiStream{handles: result}, nil
}

func (connection *Connection) Close(code uint64, reason string) error {
	connection.closeOnce.Do(func() {
		connection.closed.Store(true)
		data := []byte(reason)
		connection.closeErr = invokeJSON(context.Background(), func(functions *abi) uintptr {
			operation := functions.connectionClose(connection.handle, code, bytesPointer(data), uintptr(len(data)))
			runtime.KeepAlive(data)
			return operation
		}, nil)
	})
	return connection.closeErr
}
