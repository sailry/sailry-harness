//go:build integration

package terminal

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"strings"
	"sync"
	"testing"
	"time"

	myterminal "github.com/egoist/mygo/plugins/terminal"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestTerminalMirrorIntegration(t *testing.T) {
	if os.Getenv("SAILRY_TERMINAL_FIXTURE") == "1" {
		fmt.Print("\x1b]133;A\x07terminal fixture 中文\r\n")
		for index := range 80 {
			fmt.Printf("history %03d\r\n", index)
		}
		text, _ := bufio.NewReader(os.Stdin).ReadString('\n')
		fmt.Printf("received %s\r\n", strings.TrimSpace(text))
		os.Exit(0)
	}
	for _, encoded := range []bool{false, true} {
		t.Run(fmt.Sprintf("json_%t", encoded), func(t *testing.T) {
			ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
			defer cancel()
			node, caller := protocol.NodeID{1}, protocol.NodeID{2}
			service := New(ctx, node, nil)
			defer service.Close()
			executable, err := os.Executable()
			if err != nil {
				t.Fatal(err)
			}
			info, err := service.Start(ctx, Launch{
				ID: protocol.NewTerminalID(), Caller: caller, Worktree: protocol.NewWorktreeID(),
				Root: t.TempDir(), Command: []string{executable, "-test.run=^TestTerminalMirrorIntegration$"},
				Environment: []string{"SAILRY_TERMINAL_FIXTURE=1"}, Access: protocol.ProcessAccessUnrestricted,
				Viewport:   protocol.TerminalViewport{Columns: 80, Rows: 24},
				Appearance: protocol.TerminalAppearance{ColorScheme: protocol.TerminalColorSchemeLight},
			})
			if err != nil {
				t.Fatal(err)
			}
			stream, err := service.Subscribe(info.ID)
			if err != nil {
				t.Fatal(err)
			}
			connection := &fixtureConnection{ctx: ctx, stream: stream, service: service, caller: caller, info: info, ready: make(chan struct{}), encoded: encoded}
			client, err := myterminal.New(myterminal.Options{Conn: connection, ResizeByConn: true})
			if err != nil {
				t.Fatal(err)
			}
			defer client.Close()
			connection.terminal = client
			close(connection.ready)
			ticker := time.NewTicker(10 * time.Millisecond)
			defer ticker.Stop()
			wait := func(needle string) {
				t.Helper()
				for {
					if strings.Contains(client.Text(), needle) {
						return
					}
					select {
					case <-ctx.Done():
						t.Fatalf("terminal did not render %q: %v", needle, ctx.Err())
					case <-ticker.C:
					}
				}
			}
			wait("history 079")
			if _, err := service.Resize(caller, protocol.CommandResizeTerminalPayload{Terminal: info.ID, Revision: info.Revision, Viewport: protocol.TerminalViewport{Columns: 53, Rows: 18}}); err != nil {
				t.Fatal(err)
			}
			client.Send([]byte("ordered input 中文\n"))
			wait("received ordered input 中文")
			snapshot, err := service.Snapshot(info.ID)
			if err != nil || snapshot.Screen.VT == nil || snapshot.Screen.VT.Viewport.Columns != 53 {
				t.Fatalf("reattach snapshot: %#v %v", snapshot, err)
			}
			if _, err := service.Input(protocol.NodeID{3}, protocol.CommandInputTerminalPayload{Terminal: info.ID, Revision: info.Revision, Bytes: []byte("blocked")}); err == nil {
				t.Fatal("another controller wrote terminal input")
			}
			if _, err := service.CloseTerminal(info.Worktree, info.ID); err != nil {
				t.Fatal(err)
			}
			if len(service.List(*info.Worktree)) != 0 {
				t.Fatal("closed terminal remained in inventory")
			}
		})
	}
}

type fixtureConnection struct {
	ctx      context.Context
	stream   link.Subscription
	service  *Service
	caller   protocol.NodeID
	info     protocol.TerminalInfo
	terminal *myterminal.Terminal
	ready    chan struct{}
	encoded  bool
	pending  []byte
	close    sync.Once
}

func (connection *fixtureConnection) Read(destination []byte) (int, error) {
	select {
	case <-connection.ctx.Done():
		return 0, connection.ctx.Err()
	case <-connection.ready:
	}
	for len(connection.pending) == 0 {
		update, err := connection.stream.Next(connection.ctx)
		if err != nil {
			return 0, err
		}
		if connection.encoded {
			body, err := json.Marshal(update)
			if err != nil {
				return 0, err
			}
			if err := json.Unmarshal(body, &update); err != nil {
				return 0, err
			}
		}
		var screen protocol.TerminalScreen
		switch update.Kind {
		case "terminal_snapshot":
			value, _ := payload[protocol.TerminalSnapshot](update.Data)
			screen = value.Screen
		case "terminal_frame":
			value, _ := payload[protocol.TerminalFrame](update.Data)
			replacement, _ := payload[protocol.TerminalScreenScreenUpdateReplacePayload](value.Screen.Data)
			screen = replacement.Screen
		}
		if screen.VT == nil {
			return 0, io.ErrUnexpectedEOF
		}
		connection.terminal.Resize(int(screen.VT.Viewport.Columns), int(screen.VT.Viewport.Rows))
		connection.pending = screen.VT.Bytes
	}
	count := copy(destination, connection.pending)
	connection.pending = connection.pending[count:]
	return count, nil
}

func (connection *fixtureConnection) Write(body []byte) (int, error) {
	_, err := connection.service.Input(connection.caller, protocol.CommandInputTerminalPayload{Terminal: connection.info.ID, Revision: connection.info.Revision, Bytes: body})
	if err != nil {
		return 0, err
	}
	return len(body), nil
}

func (connection *fixtureConnection) Close() error {
	var err error
	connection.close.Do(func() { err = connection.stream.Close() })
	return err
}
