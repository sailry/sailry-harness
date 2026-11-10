//go:build integration

package main

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/link/iroh"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const fixtureEnvironment = "B7_LINK_FIXTURE"
const fixtureValue = "paired-and-controlled"

func main() {
	native := flag.String("native", "", "built native iroh library")
	controller := flag.String("controller", "", "private isolated controller directory")
	ticket := flag.String("ticket-file", "", "private Rust Host bootstrap ticket")
	mode := flag.String("mode", "pair", "pair, control, resume or relay")
	relayConfig := flag.String("relay-config", "", "private local relay fixture JSON")
	bind := flag.String("bind", "", "controller direct UDP bind address")
	internet := flag.Bool("internet", false, "controller default relay network")
	flag.Parse()
	if err := run(*native, *controller, *ticket, *mode, *relayConfig, link.NetworkScope{Bind: *bind, Internet: *internet}); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run(native, directory, ticketPath, mode, relayConfig string, scope link.NetworkScope) error {
	if native == "" || !filepath.IsAbs(directory) {
		return errors.New("native library and absolute private controller directory are required")
	}
	if mode != "pair" && mode != "control" && mode != "resume" && mode != "relay" {
		return errors.New("mode must be pair, control, resume or relay")
	}
	if err := iroh.Load(native); err != nil {
		return err
	}
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
	defer cancel()
	if mode == "relay" {
		return runRelay(ctx, directory, relayConfig)
	}
	controller, err := link.OpenController(ctx, directory, scope)
	if err != nil {
		return err
	}
	defer controller.Close()
	if mode == "resume" {
		peers, err := controller.Peers(ctx)
		if err != nil {
			return err
		}
		if len(peers) != 1 {
			return errors.New("one persisted fixture peer expected")
		}
		_, err = execute(ctx, controller.Remote(peers[0]), protocol.Command{Kind: "snapshot"}, "resume_snapshot")
		if err == nil {
			emit(map[string]any{"stage": "process_resume", "target": peers[0].ID.String()})
		}
		return err
	}
	var address link.EndpointAddress
	if mode == "control" {
		peers, err := controller.Peers(ctx)
		if err != nil {
			return err
		}
		if len(peers) != 1 {
			return errors.New("one persisted fixture peer expected")
		}
		address = peers[0]
	} else {
		metadata, err := os.Lstat(ticketPath)
		if err != nil {
			return err
		}
		if !metadata.Mode().IsRegular() || metadata.Mode().Perm()&0o077 != 0 || metadata.Size() > 8192 {
			return errors.New("private bootstrap ticket file expected")
		}
		encoded, err := os.ReadFile(ticketPath)
		if err != nil {
			return err
		}
		address, err = controller.Pair(ctx, string(encoded))
		clear(encoded)
		if err != nil {
			return err
		}
	}
	local, err := controller.Address(ctx)
	if err != nil {
		return err
	}
	emit(map[string]any{"stage": "paired", "controller": local.ID.String(), "target": address.ID.String()})
	remote := controller.Remote(address)
	output, err := execute(ctx, remote, protocol.Command{Kind: "snapshot"}, "initial_snapshot")
	if err != nil {
		return err
	}
	if output.Kind != "snapshot" {
		return errors.New("Rust snapshot output expected")
	}
	info, err := controller.Inspect(ctx, address)
	if err != nil {
		return err
	}
	if !info.Execution {
		return errors.New("Rust Host must advertise execution")
	}
	emit(map[string]any{"stage": "inspected", "software_version": info.SoftwareVersion, "protocol": info.Protocol, "execution": info.Execution})
	settings, err := readSettings(ctx, remote, "initial_settings")
	if err != nil {
		return err
	}
	original := settings
	changed := settings
	changed.Environment = make(map[string]string, len(settings.Environment)+1)
	for key, value := range settings.Environment {
		changed.Environment[key] = value
	}
	if _, exists := changed.Environment[fixtureEnvironment]; exists {
		return errors.New("isolated fixture environment key is already present")
	}
	changed.Environment[fixtureEnvironment] = fixtureValue
	subscription, err := remote.Subscribe(ctx, protocol.Topic{Kind: "node"})
	if err != nil {
		return err
	}
	defer subscription.Close()
	initial, err := subscription.Next(ctx)
	if err != nil {
		return err
	}
	if initial.Kind != "snapshot" {
		return errors.New("initial Node subscription snapshot expected")
	}
	saveRequest := protocol.NewRequest(remote.Target(), protocol.Command{Kind: "save_terminal_settings", Data: changed})
	savedOutput, err := executeRequest(ctx, remote, saveRequest, "save_settings")
	if err != nil {
		return err
	}
	saved, err := terminalSettings(savedOutput)
	if err != nil || saved.Revision != original.Revision+1 || saved.Environment[fixtureEnvironment] != fixtureValue {
		return errors.New("revision-checked Rust settings mutation was not observed")
	}
	for {
		update, err := subscription.Next(ctx)
		if err != nil {
			return err
		}
		if update.Kind != "event" {
			continue
		}
		envelope, ok := update.Data.(*protocol.EventEnvelope)
		if !ok || envelope.Event.Kind != "terminal_settings_changed" {
			continue
		}
		event, ok := envelope.Event.Data.(*protocol.EventTerminalSettingsChangedPayload)
		if !ok || envelope.Node != address.ID || event.Revision != saved.Revision {
			return errors.New("settings subscription event did not match the mutation")
		}
		emit(map[string]any{"stage": "subscription_observed", "cursor": envelope.Cursor, "revision": event.Revision})
		break
	}
	if err := subscription.Close(); err != nil {
		return err
	}
	if err := controller.Close(); err != nil {
		return err
	}
	reconnected, err := link.OpenController(ctx, directory, link.NetworkScope{})
	if err != nil {
		return err
	}
	defer reconnected.Close()
	identity, err := reconnected.Address(ctx)
	if err != nil || identity.ID != local.ID {
		return errors.New("persisted controller identity changed")
	}
	peers, err := reconnected.Peers(ctx)
	if err != nil {
		return err
	}
	if len(peers) != 1 || peers[0].ID != address.ID {
		return errors.New("persisted paired Rust Host was not restored")
	}
	remote = reconnected.Remote(peers[0])
	recovered, err := readSettings(ctx, remote, "reconnected_settings")
	if err != nil {
		return err
	}
	if !reflect.DeepEqual(saved, recovered) {
		return errors.New("Rust Host state changed across controller reconnect")
	}
	emit(map[string]any{"stage": "reconnected", "controller": identity.ID.String(), "target": peers[0].ID.String(), "revision": recovered.Revision})
	replayedOutput, err := executeRequest(ctx, remote, saveRequest, "replay_saved_request")
	if err != nil {
		return err
	}
	replayed, err := terminalSettings(replayedOutput)
	if err != nil || !reflect.DeepEqual(replayed, saved) {
		return errors.New("original saved request did not replay its exact outcome")
	}
	afterReplay, err := readSettings(ctx, remote, "read_after_replay")
	if err != nil || !reflect.DeepEqual(afterReplay, saved) {
		return errors.New("original request replay changed the Rust Host state")
	}
	emit(map[string]any{"stage": "original_request_replayed", "request": saveRequest.ID.String(), "revision": afterReplay.Revision})
	original.Revision = recovered.Revision
	restoredOutput, err := execute(ctx, remote, protocol.Command{Kind: "save_terminal_settings", Data: original}, "restore_settings")
	if err != nil {
		return err
	}
	restored, err := terminalSettings(restoredOutput)
	if err != nil {
		return err
	}
	original.Revision++
	if !reflect.DeepEqual(original, restored) {
		return errors.New("fixture settings were not restored")
	}
	emit(map[string]any{"stage": "restored", "revision": restored.Revision})
	return nil
}

func execute(ctx context.Context, remote *link.Remote, command protocol.Command, stage string) (protocol.Output, error) {
	request := protocol.NewRequest(remote.Target(), command)
	return executeRequest(ctx, remote, request, stage)
}

func executeRequest(ctx context.Context, remote *link.Remote, request protocol.Request, stage string) (protocol.Output, error) {
	admission, err := remote.Dispatch(ctx, request)
	if err != nil {
		return protocol.Output{}, err
	}
	if admission.Receipt.ID != request.ID || admission.Receipt.Durable != request.Command.Durable() {
		return protocol.Output{}, errors.New("Rust receipt did not match the request")
	}
	emit(map[string]any{"stage": stage, "received": "receipt", "request": request.ID.String(), "durable": admission.Receipt.Durable})
	select {
	case <-ctx.Done():
		return protocol.Output{}, ctx.Err()
	case response, open := <-admission.Completion:
		if !open {
			return protocol.Output{}, errors.New("Rust completion is unavailable")
		}
		if response.Fault != nil {
			return protocol.Output{}, response.Fault
		}
		emit(map[string]any{"stage": stage, "received": "completed", "request": request.ID.String(), "output": response.Output.Kind})
		return response.Output, nil
	}
}

func readSettings(ctx context.Context, remote *link.Remote, stage string) (protocol.TerminalSettings, error) {
	output, err := execute(ctx, remote, protocol.Command{Kind: "read_terminal_settings"}, stage)
	if err != nil {
		return protocol.TerminalSettings{}, err
	}
	return terminalSettings(output)
}

func terminalSettings(output protocol.Output) (protocol.TerminalSettings, error) {
	if output.Kind != "terminal_settings" {
		return protocol.TerminalSettings{}, errors.New("terminal settings output expected")
	}
	switch settings := output.Data.(type) {
	case *protocol.TerminalSettings:
		return *settings, nil
	case protocol.TerminalSettings:
		return settings, nil
	default:
		return protocol.TerminalSettings{}, errors.New("typed terminal settings output expected")
	}
}

func emit(value map[string]any) {
	_ = json.NewEncoder(os.Stdout).Encode(value)
}
