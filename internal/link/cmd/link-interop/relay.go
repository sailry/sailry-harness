//go:build integration

package main

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"sync"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/link/iroh"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type relayLedger struct {
	mu        sync.Mutex
	node      protocol.NodeID
	settings  protocol.TerminalSettings
	requests  map[protocol.RequestID]link.Response
	mutations uint64
}

func (ledger *relayLedger) Open(context.Context, protocol.NodeID, protocol.StreamID) (link.Stream, error) {
	return nil, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "fixture stream is unavailable"}
}

func (ledger *relayLedger) Subscribe(context.Context, protocol.NodeID, protocol.Topic) (link.Subscription, error) {
	return nil, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "fixture subscription is unavailable"}
}

func (ledger *relayLedger) Dispatch(ctx context.Context, _ protocol.NodeID, request protocol.Request) (link.Admission, error) {
	if err := ctx.Err(); err != nil {
		return link.Admission{}, err
	}
	ledger.mu.Lock()
	defer ledger.mu.Unlock()
	if request.Target != ledger.node {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "fixture Node target mismatch"}
	}
	response, found := ledger.requests[request.ID]
	if !found {
		switch request.Command.Kind {
		case "snapshot":
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node, snapshot.Cursor = ledger.node, ledger.mutations
			snapshot.TerminalSettingsRevision = ledger.settings.Revision
			response.Output = protocol.Output{Kind: "snapshot", Data: snapshot}
		case "save_terminal_settings":
			settings, ok := request.Command.Data.(*protocol.TerminalSettings)
			if !ok {
				return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "typed fixture settings expected"}
			}
			if settings.Revision != ledger.settings.Revision {
				return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "fixture settings revision changed"}
			}
			ledger.settings = *settings
			ledger.settings.Revision++
			ledger.mutations++
			response.Output = protocol.Output{Kind: "terminal_settings", Data: ledger.settings}
		default:
			return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "unknown fixture command"}
		}
		ledger.requests[request.ID] = response
	}
	completion := make(chan link.Response, 1)
	completion <- response
	close(completion)
	return link.Admission{
		Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completion,
	}, nil
}

func runRelay(ctx context.Context, directory, configPath string) error {
	if configPath == "" {
		return errors.New("private relay fixture configuration file is required")
	}
	metadata, err := os.Lstat(configPath)
	if err != nil {
		return err
	}
	if !metadata.Mode().IsRegular() || metadata.Mode().Perm()&0o077 != 0 || metadata.Size() > 1024*1024 {
		return errors.New("private local relay fixture configuration expected")
	}
	encoded, err := os.ReadFile(configPath)
	if err != nil {
		return err
	}
	var config struct {
		URL      string   `json:"url"`
		RootsDER [][]byte `json:"roots_der"`
	}
	err = json.Unmarshal(encoded, &config)
	clear(encoded)
	if err != nil || config.URL == "" || len(config.RootsDER) == 0 {
		return errors.New("relay URL and fixture certificate roots are required")
	}
	ledger := &relayLedger{
		settings: protocol.DefaultTerminalSettings(),
		requests: make(map[protocol.RequestID]link.Response),
	}
	server, err := link.OpenRelayFixture(ctx, filepath.Join(directory, "server"), config.URL, ledger, config.RootsDER)
	if err != nil {
		return err
	}
	defer server.Close()
	controller, err := link.OpenRelayFixture(ctx, filepath.Join(directory, "controller"), config.URL, nil, config.RootsDER)
	if err != nil {
		return err
	}
	defer controller.Close()
	if err := server.Online(ctx); err != nil {
		return err
	}
	if err := controller.Online(ctx); err != nil {
		return err
	}
	serverAddress, err := server.Address(ctx)
	if err != nil {
		return err
	}
	ledger.mu.Lock()
	ledger.node = serverAddress.ID
	ledger.mu.Unlock()
	controllerAddress, err := controller.Address(ctx)
	if err != nil {
		return err
	}
	invitation, err := server.Invite(ctx)
	if err != nil {
		return err
	}
	defer invitation.Close()
	paired, err := controller.Pair(ctx, invitation.Ticket())
	if err != nil {
		return err
	}
	remote := controller.Remote(paired)
	if _, err := execute(ctx, remote, protocol.Command{Kind: "snapshot"}, "relay_initial_snapshot"); err != nil {
		return err
	}
	info, err := remote.FixtureConnectionInfo(ctx)
	if err != nil || info.Route != "relay" || info.RemoteID != serverAddress.ID {
		return errors.New("forced relay authenticated path was not selected")
	}
	changed := protocol.DefaultTerminalSettings()
	changed.Environment[fixtureEnvironment] = fixtureValue
	request := protocol.NewRequest(remote.Target(), protocol.Command{Kind: "save_terminal_settings", Data: changed})
	emit(map[string]any{
		"stage": "relay_ready", "route": info.Route, "controller": controllerAddress.ID.String(),
		"target": serverAddress.ID.String(), "request": request.ID.String(), "remote_retained": true,
	})
	input := bufio.NewScanner(os.Stdin)
	input.Buffer(make([]byte, 4096), 8192)
	interrupted := false
	for input.Scan() {
		var command struct {
			Op string `json:"op"`
		}
		if err := json.Unmarshal(input.Bytes(), &command); err != nil {
			return errors.New("fixture JSON operation expected")
		}
		switch command.Op {
		case "outage":
			if interrupted {
				return errors.New("outage was already observed")
			}
			_, err := executeRequest(ctx, remote, request, "relay_outage_request")
			if err == nil {
				return errors.New("stopped relay unexpectedly completed its fixture request")
			}
			var fault *protocol.Fault
			if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeOutcomeUnknown && fault.Code != protocol.ErrorCodeUnavailable {
				return err
			}
			interrupted = true
			emit(map[string]any{"stage": "relay_outage_observed", "fault": fault.Code, "request": request.ID.String()})
		case "recover":
			if !interrupted {
				return errors.New("stop the relay and observe its outage before recovery")
			}
			output, err := executeRequest(ctx, remote, request, "relay_recovery_request")
			if err != nil {
				var fault *protocol.Fault
				if errors.As(err, &fault) && (fault.Code == protocol.ErrorCodeOutcomeUnknown || fault.Code == protocol.ErrorCodeUnavailable) {
					emit(map[string]any{"stage": "relay_recovery_pending", "fault": fault.Code, "request": request.ID.String()})
					continue
				}
				return err
			}
			saved, err := terminalSettings(output)
			if err != nil || saved.Revision != 1 || saved.Environment[fixtureEnvironment] != fixtureValue {
				return errors.New("recovered original request returned an unexpected outcome")
			}
			replayedOutput, err := executeRequest(ctx, remote, request, "relay_replay_request")
			if err != nil {
				return err
			}
			replayed, err := terminalSettings(replayedOutput)
			if err != nil || !reflect.DeepEqual(replayed, saved) {
				return errors.New("relay recovery replay changed its committed outcome")
			}
			ledger.mu.Lock()
			mutations := ledger.mutations
			ledger.mu.Unlock()
			current, err := controller.Address(ctx)
			if err != nil || current.ID != controllerAddress.ID || remote.Target() != serverAddress.ID || mutations != 1 {
				return errors.New("relay recovery changed its identity, Remote target or admission count")
			}
			info, err := remote.FixtureConnectionInfo(ctx)
			if err != nil || info.Route != "relay" || info.RemoteID != serverAddress.ID {
				return errors.New("recovered Link did not retain its authenticated relay route")
			}
			emit(map[string]any{
				"stage": "relay_recovered", "request": request.ID.String(), "route": info.Route,
				"controller": current.ID.String(), "target": remote.Target().String(),
				"remote_retained": true, "mutations": mutations,
			})
			if err := errors.Join(controller.Close(), server.Close()); err != nil {
				return err
			}
			counts := iroh.DebugCounts()
			if counts != (iroh.Counts{}) {
				return errors.New("Link relay fixture retained native resources after close")
			}
			emit(map[string]any{"stage": "relay_closed", "counts": counts})
			return nil
		default:
			return errors.New("fixture operation must be outage or recover")
		}
	}
	if err := input.Err(); err != nil {
		return err
	}
	return errors.New("relay fixture ended before recovery")
}
