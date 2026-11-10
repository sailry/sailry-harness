package iroh

import (
	"context"
	"encoding/json"
	"runtime"
	"sync"
	"sync/atomic"
)

type Config struct {
	SecretKey [32]byte `json:"secret_key"`
	Mode      string   `json:"mode"`
	Bind      string   `json:"bind,omitempty"`
	Relays    []string `json:"relays,omitempty"`
	RootsDER  [][]byte `json:"roots_der,omitempty"`
	RelayOnly bool     `json:"relay_only,omitempty"`
}

func (Config) String() string   { return "iroh.Config([REDACTED])" }
func (Config) GoString() string { return "iroh.Config([REDACTED])" }

type Endpoint struct {
	handle    uint64
	closed    atomic.Bool
	closeOnce sync.Once
	closeErr  error
}

type RelayStatus struct {
	URL              string `json:"url"`
	Connected        bool   `json:"connected"`
	Error            string `json:"error"`
	AuthDeniedReason string `json:"auth_denied_reason"`
}

func DefaultRelayURLs(ctx context.Context) ([]string, error) {
	var relays []string
	err := invokeJSON(ctx, func(functions *abi) uintptr {
		return functions.defaultRelayURLs()
	}, &relays)
	return relays, err
}

func NewEndpoint(ctx context.Context, config Config) (*Endpoint, error) {
	roots := make([][]uint16, len(config.RootsDER))
	for index, root := range config.RootsDER {
		roots[index] = make([]uint16, len(root))
		for offset, value := range root {
			roots[index][offset] = uint16(value)
		}
	}
	data, err := json.Marshal(struct {
		SecretKey [32]byte   `json:"secret_key"`
		Mode      string     `json:"mode"`
		Bind      string     `json:"bind,omitempty"`
		Relays    []string   `json:"relays,omitempty"`
		RootsDER  [][]uint16 `json:"roots_der,omitempty"`
		RelayOnly bool       `json:"relay_only,omitempty"`
	}{config.SecretKey, config.Mode, config.Bind, config.Relays, roots, config.RelayOnly})
	if err != nil {
		return nil, err
	}
	var result struct {
		Handle uint64 `json:"handle"`
	}
	err = invokeJSON(ctx, func(functions *abi) uintptr {
		operation := functions.endpointNew(bytesPointer(data), uintptr(len(data)))
		runtime.KeepAlive(data)
		return operation
	}, &result)
	clear(data)
	if err != nil {
		return nil, err
	}
	return &Endpoint{handle: result.Handle}, nil
}

func (endpoint *Endpoint) Address(ctx context.Context) (Address, error) {
	if endpoint.closed.Load() {
		return Address{}, ErrClosed
	}
	var address Address
	err := invokeJSON(ctx, func(functions *abi) uintptr {
		return functions.endpointAddress(endpoint.handle)
	}, &address)
	return address, err
}

func (endpoint *Endpoint) Online(ctx context.Context) error {
	if endpoint.closed.Load() {
		return ErrClosed
	}
	return invokeJSON(ctx, func(functions *abi) uintptr {
		return functions.endpointOnline(endpoint.handle)
	}, nil)
}

func (endpoint *Endpoint) NetworkChange(ctx context.Context) error {
	if endpoint.closed.Load() {
		return ErrClosed
	}
	return invokeJSON(ctx, func(functions *abi) uintptr {
		return functions.endpointNetworkChange(endpoint.handle)
	}, nil)
}

func (endpoint *Endpoint) InsertRelay(ctx context.Context, url string) error {
	if endpoint.closed.Load() {
		return ErrClosed
	}
	data := []byte(url)
	return invokeJSON(ctx, func(functions *abi) uintptr {
		operation := functions.endpointInsertRelay(endpoint.handle, bytesPointer(data), uintptr(len(data)))
		runtime.KeepAlive(data)
		return operation
	}, nil)
}

func (endpoint *Endpoint) RemoveRelay(ctx context.Context, url string) error {
	if endpoint.closed.Load() {
		return ErrClosed
	}
	data := []byte(url)
	return invokeJSON(ctx, func(functions *abi) uintptr {
		operation := functions.endpointRemoveRelay(endpoint.handle, bytesPointer(data), uintptr(len(data)))
		runtime.KeepAlive(data)
		return operation
	}, nil)
}

func (endpoint *Endpoint) RelayStatus(ctx context.Context) ([]RelayStatus, error) {
	if endpoint.closed.Load() {
		return nil, ErrClosed
	}
	var statuses []RelayStatus
	err := invokeJSON(ctx, func(functions *abi) uintptr {
		return functions.endpointRelayStatus(endpoint.handle)
	}, &statuses)
	return statuses, err
}

func (endpoint *Endpoint) WaitRelayStatus(ctx context.Context, previous []RelayStatus) ([]RelayStatus, error) {
	if endpoint.closed.Load() {
		return nil, ErrClosed
	}
	if previous == nil {
		previous = []RelayStatus{}
	}
	data, err := json.Marshal(previous)
	if err != nil {
		return nil, err
	}
	var statuses []RelayStatus
	err = invokeJSON(ctx, func(functions *abi) uintptr {
		operation := functions.endpointWaitRelayStatus(endpoint.handle, bytesPointer(data), uintptr(len(data)))
		runtime.KeepAlive(data)
		return operation
	}, &statuses)
	return statuses, err
}

func (endpoint *Endpoint) WaitAddress(ctx context.Context, previous Address) (Address, error) {
	if endpoint.closed.Load() {
		return Address{}, ErrClosed
	}
	data, err := json.Marshal(previous)
	if err != nil {
		return Address{}, err
	}
	var address Address
	err = invokeJSON(ctx, func(functions *abi) uintptr {
		operation := functions.endpointWaitAddress(endpoint.handle, bytesPointer(data), uintptr(len(data)))
		runtime.KeepAlive(data)
		return operation
	}, &address)
	return address, err
}

func (endpoint *Endpoint) Connect(ctx context.Context, address Address, alpn string) (*Connection, error) {
	if endpoint.closed.Load() {
		return nil, ErrClosed
	}
	data, err := json.Marshal(address)
	if err != nil {
		return nil, err
	}
	protocol := []byte(alpn)
	var result struct {
		Handle uint64 `json:"handle"`
	}
	err = invokeJSON(ctx, func(functions *abi) uintptr {
		operation := functions.connect(endpoint.handle, bytesPointer(data), uintptr(len(data)), bytesPointer(protocol), uintptr(len(protocol)))
		runtime.KeepAlive(data)
		runtime.KeepAlive(protocol)
		return operation
	}, &result)
	if err != nil {
		return nil, err
	}
	return &Connection{handle: result.Handle}, nil
}

func (endpoint *Endpoint) Accept(ctx context.Context) (*Connection, error) {
	if endpoint.closed.Load() {
		return nil, ErrClosed
	}
	var result struct {
		Handle uint64 `json:"handle"`
	}
	err := invokeJSON(ctx, func(functions *abi) uintptr {
		return functions.accept(endpoint.handle)
	}, &result)
	if err != nil {
		return nil, err
	}
	return &Connection{handle: result.Handle}, nil
}

func (endpoint *Endpoint) Close() error {
	endpoint.closeOnce.Do(func() {
		endpoint.closed.Store(true)
		endpoint.closeErr = invokeJSON(context.Background(), func(functions *abi) uintptr {
			return functions.endpointClose(endpoint.handle)
		}, nil)
	})
	return endpoint.closeErr
}
