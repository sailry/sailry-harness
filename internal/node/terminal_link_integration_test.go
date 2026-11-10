//go:build integration

package node

import (
	"bufio"
	"bytes"
	"context"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestMain(suite *testing.M) {
	if name := strings.TrimSuffix(filepath.Base(os.Args[0]), ".exe"); name == "codex" {
		fmt.Print("\x1b]2;ready\x07\x1b]133;A\x07fixture 中文\r\n")
		scanner := bufio.NewScanner(os.Stdin)
		for scanner.Scan() {
			fmt.Printf("received %s\r\n", scanner.Text())
			if scanner.Text() == "exit" {
				break
			}
		}
		os.Exit(0)
	}
	os.Exit(suite.Run())
}

func TestTerminalControllerIntegration(test *testing.T) {
	executable, err := os.Executable()
	if err != nil {
		test.Fatal(err)
	}
	bin := test.TempDir()
	name := "codex"
	if runtime.GOOS == "windows" {
		name += ".exe"
	}
	source, err := os.Open(executable)
	if err != nil {
		test.Fatal(err)
	}
	defer source.Close()
	destination, err := os.OpenFile(filepath.Join(bin, name), os.O_CREATE|os.O_WRONLY, 0o700)
	if err != nil {
		test.Fatal(err)
	}
	if _, err := io.Copy(destination, source); err != nil {
		destination.Close()
		test.Fatal(err)
	}
	if err := destination.Close(); err != nil {
		test.Fatal(err)
	}
	for _, remote := range []bool{false, true} {
		test.Run(fmt.Sprintf("link_%t", remote), func(test *testing.T) {
			ctx, cancel := context.WithTimeout(test.Context(), 30*time.Second)
			defer cancel()
			scope := link.NetworkScope{Bind: "127.0.0.1:0"}
			environment := process.NewEnvironment([]string{"PATH=" + bin})
			owner, err := StartWithOptions(ctx, filepath.Join(test.TempDir(), "node"), Options{Network: &scope, Environment: &environment, MCP: fixtureMCPOptions()})
			if err != nil {
				test.Fatal(err)
			}
			defer owner.Stop(context.Background())
			session := sessionFixture(test, owner)
			var transport link.Transport = link.NewLocal(owner.ID(), owner.ID(), owner)
			if remote {
				endpoint, err := link.OpenController(ctx, filepath.Join(test.TempDir(), "controller"), scope)
				if err != nil {
					test.Fatal(err)
				}
				defer endpoint.Close()
				controllerAddress, err := endpoint.Address(ctx)
				if err != nil {
					test.Fatal(err)
				}
				if err := owner.Link().SetTrust(ctx, controllerAddress.ID, true); err != nil {
					test.Fatal(err)
				}
				if err := endpoint.SetTrust(ctx, owner.ID(), true); err != nil {
					test.Fatal(err)
				}
				nodeAddress, err := owner.Link().Address(ctx)
				if err != nil {
					test.Fatal(err)
				}
				transport = endpoint.Remote(nodeAddress)
			}
			controller := client.New(transport)
			defer controller.Close()
			if err := controller.Start(ctx); err != nil {
				test.Fatal(err)
			}
			output, err := controller.Do(ctx, protocol.Command{Kind: "list_terminal_tools", Data: protocol.CommandListTerminalToolsPayload{Worktree: session.Worktree}})
			if err != nil || output.Kind != "terminal_tools" {
				test.Fatalf("CLI discovery: %s %v", output.Kind, err)
			}
			launch := protocol.TerminalLaunch{Worktree: session.Worktree, Access: protocol.ProcessAccessUnrestricted, Viewport: protocol.TerminalViewport{Columns: 80, Rows: 24}, Appearance: protocol.TerminalAppearance{ColorScheme: protocol.TerminalColorSchemeLight}}
			request := controller.Prepare(protocol.Command{Kind: "open_tool_terminal", Data: protocol.CommandOpenToolTerminalPayload{Tool: protocol.TerminalToolsToolCodex, Launch: launch}})
			output, err = controller.Execute(ctx, request)
			info, valid := dispatchPayload[protocol.TerminalInfo](output.Data)
			if err != nil || output.Kind != "terminal" || !valid {
				test.Fatalf("CLI launch: %s %v", output.Kind, err)
			}
			replay, err := controller.Execute(ctx, request)
			replayed, valid := dispatchPayload[protocol.TerminalInfo](replay.Data)
			if err != nil || !valid || replayed.ID != info.ID || len(owner.terminals.List(session.Worktree)) != 1 {
				test.Fatalf("CLI receipt replay: %v", err)
			}
			stream, err := controller.Subscribe(ctx, protocol.Topic{Kind: "terminal", Data: info.ID})
			if err != nil {
				test.Fatal(err)
			}
			defer stream.Close()
			var received bytes.Buffer
			wait := func(needle string, columns uint16) {
				test.Helper()
				for {
					update, err := stream.Next(ctx)
					if err != nil {
						test.Fatal(err)
					}
					var screen protocol.TerminalScreen
					switch update.Kind {
					case "terminal_snapshot":
						snapshot, valid := dispatchPayload[protocol.TerminalSnapshot](update.Data)
						if !valid || snapshot.Node != owner.ID() {
							test.Fatal("terminal snapshot changed its owner")
						}
						screen = snapshot.Screen
					case "terminal_frame":
						frame, valid := dispatchPayload[protocol.TerminalFrame](update.Data)
						if !valid || frame.Node != owner.ID() {
							test.Fatal("terminal frame changed its owner")
						}
						replacement, _ := dispatchPayload[protocol.TerminalScreenScreenUpdateReplacePayload](frame.Screen.Data)
						screen = replacement.Screen
					}
					if screen.VT == nil {
						test.Fatal("terminal mirror payload is missing")
					}
					received.Write(screen.VT.Bytes)
					if strings.Contains(received.String(), needle) && screen.VT.Viewport.Columns == columns {
						return
					}
				}
			}
			wait("fixture 中文", 80)
			if _, err := controller.Do(ctx, protocol.Command{Kind: "resize_terminal", Data: protocol.CommandResizeTerminalPayload{Terminal: info.ID, Revision: info.Revision, Viewport: protocol.TerminalViewport{Columns: 53, Rows: 18}}}); err != nil {
				test.Fatal(err)
			}
			if _, err := controller.Do(ctx, protocol.Command{Kind: "input_terminal", Data: protocol.CommandInputTerminalPayload{Terminal: info.ID, Revision: info.Revision, Bytes: []byte("ordered 中文\n")}}); err != nil {
				test.Fatal(err)
			}
			wait("received ordered 中文", 53)
			_ = stream.Close()
			stream, err = controller.Subscribe(ctx, protocol.Topic{Kind: "terminal", Data: info.ID})
			if err != nil {
				test.Fatal(err)
			}
			defer stream.Close()
			received.Reset()
			wait("received ordered 中文", 53)
			other := link.NewLocal(owner.ID(), protocol.NodeID{9}, owner)
			otherController := client.New(other)
			defer otherController.Close()
			if _, err := otherController.Do(ctx, protocol.Command{Kind: "input_terminal", Data: protocol.CommandInputTerminalPayload{Terminal: info.ID, Revision: info.Revision, Bytes: []byte("blocked\n")}}); err == nil {
				test.Fatal("another controller wrote terminal input")
			}
			if _, err := controller.Do(ctx, protocol.Command{Kind: "close_terminal", Data: protocol.CommandCloseTerminalPayload{Worktree: session.Worktree, Terminal: info.ID}}); err != nil {
				test.Fatal(err)
			}
			snapshot, err := owner.Snapshot(ctx)
			if err != nil || len(snapshot.Terminals) != 1 || snapshot.Terminals[0].Status.Kind != "closed" {
				test.Fatalf("terminal close inventory: %v %v", snapshot.Terminals, err)
			}
		})
	}
}
