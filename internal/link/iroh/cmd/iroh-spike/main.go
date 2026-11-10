//go:build integration

package main

import (
	"bufio"
	"context"
	"crypto/rand"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"runtime"
	"time"

	"github.com/sailry/sailry-harness/internal/link/iroh"
)

type command struct {
	Op         string       `json:"op"`
	Address    iroh.Address `json:"address"`
	ALPN       string       `json:"alpn"`
	Connection uint64       `json:"connection"`
	Stream     uint64       `json:"stream"`
	Data       string       `json:"data"`
	Maximum    int          `json:"maximum"`
	Code       uint64       `json:"code"`
	Reason     string       `json:"reason"`
}

type fixture struct {
	endpoint    *iroh.Endpoint
	connections map[uint64]*iroh.Connection
	streams     map[uint64]*iroh.BiStream
	next        uint64
}

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run() error {
	libraryPath := flag.String("library", "", "Native iroh library path")
	configPath := flag.String("config", "", "Private fixture configuration path")
	flag.Parse()
	if *libraryPath == "" {
		return errors.New("native library path is required")
	}
	if err := iroh.Load(*libraryPath); err != nil {
		return err
	}
	config := iroh.Config{Mode: "direct", Bind: "127.0.0.1:0"}
	if *configPath != "" {
		data, err := os.ReadFile(*configPath)
		if err != nil {
			return err
		}
		err = json.Unmarshal(data, &config)
		clear(data)
		if err != nil {
			return err
		}
	}
	if config.SecretKey == [32]byte{} {
		if _, err := rand.Read(config.SecretKey[:]); err != nil {
			return err
		}
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	endpoint, err := iroh.NewEndpoint(ctx, config)
	cancel()
	if err != nil {
		return err
	}
	defer endpoint.Close()
	fixture := &fixture{
		endpoint:    endpoint,
		connections: make(map[uint64]*iroh.Connection),
		streams:     make(map[uint64]*iroh.BiStream),
	}
	address, err := endpoint.Address(context.Background())
	if err != nil {
		return err
	}
	if err := publish(map[string]any{"address": address}); err != nil {
		return err
	}
	input := bufio.NewScanner(os.Stdin)
	input.Buffer(make([]byte, 8192), 1024*1024)
	for input.Scan() {
		var command command
		if err := json.Unmarshal(input.Bytes(), &command); err != nil {
			if err := publish(map[string]any{"error": err.Error()}); err != nil {
				return err
			}
			continue
		}
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		value, err := fixture.invoke(ctx, command)
		cancel()
		if err != nil {
			value = map[string]any{"error": err.Error()}
		}
		if err := publish(value); err != nil {
			return err
		}
		if command.Op == "close" {
			return nil
		}
	}
	return input.Err()
}

func publish(value any) error {
	return json.NewEncoder(os.Stdout).Encode(value)
}

func (fixture *fixture) identifier() uint64 {
	fixture.next++
	return fixture.next
}

func (fixture *fixture) invoke(ctx context.Context, command command) (any, error) {
	switch command.Op {
	case "address":
		address, err := fixture.endpoint.Address(ctx)
		return map[string]any{"address": address}, err
	case "online":
		if err := fixture.endpoint.Online(ctx); err != nil {
			return nil, err
		}
		address, err := fixture.endpoint.Address(ctx)
		return map[string]any{"address": address}, err
	case "network_change":
		return map[string]any{"changed": true}, fixture.endpoint.NetworkChange(ctx)
	case "relay_status":
		status, err := fixture.endpoint.RelayStatus(ctx)
		return map[string]any{"status": status}, err
	case "connect", "accept":
		var connection *iroh.Connection
		var err error
		if command.Op == "accept" {
			connection, err = fixture.endpoint.Accept(ctx)
		} else {
			alpn := command.ALPN
			if alpn == "" {
				alpn = "sailry/node/1"
			}
			connection, err = fixture.endpoint.Connect(ctx, command.Address, alpn)
		}
		if err != nil {
			return nil, err
		}
		identifier := fixture.identifier()
		fixture.connections[identifier] = connection
		info, err := connection.Info(ctx)
		return map[string]any{"connection": identifier, "info": info}, err
	case "open", "accept_bi":
		connection := fixture.connections[command.Connection]
		if connection == nil {
			return nil, errors.New("fixture connection not found")
		}
		var stream *iroh.BiStream
		var err error
		if command.Op == "open" {
			stream, err = connection.OpenBi(ctx)
		} else {
			stream, err = connection.AcceptBi(ctx)
		}
		if err != nil {
			return nil, err
		}
		identifier := fixture.identifier()
		fixture.streams[identifier] = stream
		return map[string]any{"stream": identifier}, nil
	case "info":
		connection := fixture.connections[command.Connection]
		if connection == nil {
			return nil, errors.New("fixture connection not found")
		}
		info, err := connection.Info(ctx)
		return map[string]any{"info": info}, err
	case "close_connection":
		connection := fixture.connections[command.Connection]
		if connection == nil {
			return nil, errors.New("fixture connection not found")
		}
		err := connection.Close(command.Code, command.Reason)
		delete(fixture.connections, command.Connection)
		return map[string]any{"closed": true}, err
	case "write", "read", "finish", "wait_stopped", "stop", "reset", "close_stream", "cancel_read":
		stream := fixture.streams[command.Stream]
		if stream == nil {
			return nil, errors.New("fixture stream not found")
		}
		return fixture.streamCommand(ctx, stream, command)
	case "cancel_accept":
		err := cancelled(ctx, func(ctx context.Context) error {
			_, err := fixture.endpoint.Accept(ctx)
			return err
		})
		return map[string]any{"cancelled": err == nil, "counts": iroh.DebugCounts()}, err
	case "stats":
		if err := settled(ctx); err != nil {
			return nil, err
		}
		return map[string]any{"counts": iroh.DebugCounts()}, nil
	case "close":
		err := fixture.endpoint.Close()
		if err == nil {
			err = settled(ctx)
		}
		return map[string]any{"closed": true, "counts": iroh.DebugCounts()}, err
	default:
		return nil, errors.New("unknown fixture operation")
	}
}

func (fixture *fixture) streamCommand(ctx context.Context, stream *iroh.BiStream, command command) (any, error) {
	switch command.Op {
	case "write":
		data := []byte(command.Data)
		written := 0
		for written < len(data) {
			length, err := stream.WriteContext(ctx, data[written:])
			written += length
			if err != nil {
				return map[string]any{"written": written}, err
			}
		}
		return map[string]any{"written": written}, nil
	case "read":
		maximum := command.Maximum
		if maximum <= 0 {
			maximum = 8192
		}
		data := make([]byte, maximum)
		length, err := stream.ReadContext(ctx, data)
		if errors.Is(err, io.EOF) {
			return map[string]any{"data": string(data[:length]), "eof": true}, nil
		}
		return map[string]any{"data": string(data[:length]), "eof": false}, err
	case "finish":
		return map[string]any{"finished": true}, stream.CloseWrite()
	case "wait_stopped":
		code, err := stream.WaitStopped(ctx)
		return map[string]any{"code": code}, err
	case "stop":
		return map[string]any{"stopped": true}, stream.Stop(command.Code)
	case "reset":
		return map[string]any{"reset": true}, stream.Reset(command.Code)
	case "close_stream":
		err := stream.Close()
		delete(fixture.streams, command.Stream)
		return map[string]any{"closed": true}, err
	case "cancel_read":
		err := cancelled(ctx, func(ctx context.Context) error {
			_, err := stream.ReadContext(ctx, make([]byte, 64))
			return err
		})
		return map[string]any{"cancelled": err == nil, "counts": iroh.DebugCounts()}, err
	default:
		return nil, errors.New("unknown fixture stream operation")
	}
}

func cancelled(parent context.Context, work func(context.Context) error) error {
	if err := settled(parent); err != nil {
		return err
	}
	ctx, cancel := context.WithCancel(parent)
	defer cancel()
	completed := make(chan error, 1)
	go func() { completed <- work(ctx) }()
	for iroh.DebugCounts().Operations == 0 {
		select {
		case err := <-completed:
			return fmt.Errorf("fixture operation completed before cancellation: %v", err)
		case <-parent.Done():
			return parent.Err()
		default:
			runtime.Gosched()
		}
	}
	cancel()
	if err := <-completed; !errors.Is(err, context.Canceled) {
		return fmt.Errorf("fixture cancellation returned %v", err)
	}
	return settled(parent)
}

func settled(ctx context.Context) error {
	for {
		counts := iroh.DebugCounts()
		if counts.Operations == 0 && counts.Callbacks == 0 {
			return nil
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		default:
			runtime.Gosched()
		}
	}
}
