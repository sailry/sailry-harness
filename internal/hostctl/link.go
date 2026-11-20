package hostctl

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const LinkUsage = "Usage: sailry link [--data-dir <profile>] [--pairing-service <HTTPS origin>] [--relay <preset-id|direct|custom> [--address <relay address>]]"

func RunLink(ctx context.Context, args []string, stdin *bufio.Reader, interactive bool, stdout, stderr io.Writer) error {
	if len(args) == 1 && (args[0] == "--help" || args[0] == "-h") {
		_, err := fmt.Fprintln(stdout, LinkUsage)
		return err
	}
	flags := flag.NewFlagSet("link", flag.ContinueOnError)
	flags.SetOutput(stderr)
	profile := flags.String("data-dir", "", "Host profile directory")
	origin := flags.String("pairing-service", link.DefaultPairingService, "Pairing service origin")
	relayID := flags.String("relay", "", "Network preset ID, direct, or custom")
	address := flags.String("address", "", "Relay hostname or IP with an optional port, or a libp2p multiaddr")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if flags.NArg() != 0 {
		return errors.New(LinkUsage)
	}
	if *profile == "" {
		var err error
		*profile, err = core.DefaultDataDir()
		if err != nil {
			return err
		}
	} else if !filepath.IsAbs(*profile) {
		return errors.New("--data-dir must be an absolute path")
	}
	if err := validateOrigin(*origin); err != nil {
		return err
	}
	wizard := *relayID == ""
	if wizard && !interactive {
		return errors.New("link requires a terminal or an explicit --relay; " + LinkUsage)
	}
	if !wizard && (*relayID == "custom") != (*address != "") {
		return errors.New("custom relay requires --address, and other relays do not use --address")
	}
	transport, err := Connect(ctx, *profile)
	if err != nil {
		return errors.New("Host is not running")
	}
	controller := client.New(transport)
	defer controller.Close()
	settings, err := readNetworkSettings(ctx, controller)
	if err != nil {
		return err
	}
	listing, err := readNetworkCatalog(ctx, controller)
	if err != nil {
		return err
	}
	selection, err := selectNetwork(ctx, stdin, stdout, listing, *relayID, *address, wizard)
	if err != nil {
		return err
	}
	if err := applyNetworkSelection(ctx, controller, settings, selection); err != nil {
		return err
	}
	return RunShare(ctx, []string{"--data-dir", *profile, "--pairing-service", *origin}, stdout, stderr)
}

func selectNetwork(ctx context.Context, input *bufio.Reader, output io.Writer, listing protocol.NetworkListing, relayID, address string, interactive bool) (protocol.NetworkSelection, error) {
	options := make([]protocol.NetworkPreset, 0, len(listing.Presets)+1)
	options = append(options, protocol.NetworkPreset{ID: "direct", Label: protocol.PluginDesktopNavigation{Label: "Direct only (LAN)"}, Selection: protocol.NetworkSelection{Kind: "direct"}})
	for _, preset := range listing.Presets {
		if preset.ID == "direct" || preset.ID == "custom" || preset.Selection.Kind != "custom" {
			continue
		}
		if preset.ID == "" || strings.TrimSpace(preset.Label.Label) == "" && len(preset.Label.Locales) == 0 {
			return protocol.NetworkSelection{}, errors.New("Host network catalog contains an invalid preset")
		}
		options = append(options, preset)
	}
	if relayID == "" {
		if _, err := fmt.Fprintln(output, "Select the relay network for this Host:"); err != nil {
			return protocol.NetworkSelection{}, err
		}
		for index, option := range options {
			label := networkPresetLabel(option.Label)
			if label == "" {
				label = option.ID
			}
			if _, err := fmt.Fprintf(output, "  %d  %s\n", index+1, label); err != nil {
				return protocol.NetworkSelection{}, err
			}
		}
		if _, err := fmt.Fprintf(output, "  %d  Custom relay\n", len(options)+1); err != nil {
			return protocol.NetworkSelection{}, err
		}
		value, err := promptValue(ctx, input, output, "Network: ")
		if err != nil {
			return protocol.NetworkSelection{}, err
		}
		if value == "custom" || value == fmt.Sprint(len(options)+1) {
			relayID = "custom"
		} else {
			found := false
			for index, option := range options {
				if value == option.ID || value == fmt.Sprint(index+1) {
					relayID = option.ID
					found = true
					break
				}
			}
			if !found {
				return protocol.NetworkSelection{}, errors.New("choose a listed network number or preset ID")
			}
		}
	}
	var selection protocol.NetworkSelection
	if relayID == "custom" {
		if address == "" {
			var err error
			address, err = promptValue(ctx, input, output, "Relay address (host:port): ")
			if err != nil {
				return protocol.NetworkSelection{}, err
			}
		}
		selection = protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{strings.TrimSpace(address)}}}
	} else {
		found := false
		for _, option := range options {
			if option.ID == relayID {
				selection, found = option.Selection, true
				break
			}
		}
		if !found {
			return protocol.NetworkSelection{}, fmt.Errorf("unknown relay preset %q", relayID)
		}
	}
	selection, err := link.ResolveNetworkSelection(ctx, selection, options[1:])
	if err != nil {
		return protocol.NetworkSelection{}, err
	}
	if interactive {
		if _, err := promptValue(ctx, input, output, "Press Enter to apply this network and show the pairing code: "); err != nil {
			return protocol.NetworkSelection{}, err
		}
	}
	return selection, nil
}

func promptValue(ctx context.Context, input *bufio.Reader, output io.Writer, label string) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	if _, err := io.WriteString(output, label); err != nil {
		return "", err
	}
	value, err := input.ReadString('\n')
	if ctx.Err() != nil {
		return "", ctx.Err()
	}
	if err != nil {
		return "", err
	}
	return strings.TrimSpace(value), nil
}

func networkPresetLabel(label protocol.PluginDesktopNavigation) string {
	locale := strings.ToLower(strings.ReplaceAll(os.Getenv("LANG"), "_", "-"))
	preferred := "en"
	if strings.Contains(locale, "zh") {
		preferred = "zh-cn"
	}
	for key, value := range label.Locales {
		key = strings.ToLower(strings.ReplaceAll(key, "_", "-"))
		if (key == preferred || preferred == "zh-cn" && key == "zh") && strings.TrimSpace(value) != "" {
			return strings.TrimSpace(value)
		}
	}
	return strings.TrimSpace(label.Label)
}

func readNetworkSettings(ctx context.Context, controller *client.Client) (protocol.NetworkSettings, error) {
	output, err := controller.Do(ctx, protocol.Command{Kind: "read_network_settings"})
	if err != nil {
		return protocol.NetworkSettings{}, err
	}
	if output.Kind != "network_settings" {
		return protocol.NetworkSettings{}, errors.New("Host returned unexpected network settings")
	}
	return payload[protocol.NetworkSettings](output.Data)
}

func readNetworkCatalog(ctx context.Context, controller *client.Client) (protocol.NetworkListing, error) {
	output, err := controller.Do(ctx, protocol.Command{Kind: "list_network_presets", Data: protocol.CommandListNetworkPresetsPayload{Refresh: true}})
	if err != nil {
		return protocol.NetworkListing{}, err
	}
	if output.Kind != "network_presets" {
		return protocol.NetworkListing{}, errors.New("Host returned unexpected network presets")
	}
	return payload[protocol.NetworkListing](output.Data)
}

func applyNetworkSelection(ctx context.Context, controller *client.Client, current protocol.NetworkSettings, selection protocol.NetworkSelection) error {
	if equalNetworkSelection(current.Selection, &selection) && equalNetworkSelection(current.Effective, &selection) {
		return nil
	}
	command := protocol.Command{Kind: "save_network_settings", Data: protocol.CommandSaveNetworkSettingsPayload{ExpectedRevision: current.Revision, Selection: selection}}
	request := controller.Prepare(command)
	output, err := controller.Settle(ctx, request)
	if err != nil {
		observed, readErr := readNetworkSettings(ctx, controller)
		if readErr == nil && equalNetworkSelection(observed.Selection, &selection) && equalNetworkSelection(observed.Effective, &selection) {
			return nil
		}
		if client.Retained(err) {
			return fmt.Errorf("network change is unresolved; keep request %s and inspect Host status: %w", request.ID, err)
		}
		return err
	}
	if output.Kind != "network_settings" {
		return errors.New("Host returned unexpected network change result")
	}
	actual, err := payload[protocol.NetworkSettings](output.Data)
	if err != nil {
		return err
	}
	if !equalNetworkSelection(actual.Selection, &selection) || !equalNetworkSelection(actual.Effective, &selection) {
		return errors.New("Host network did not switch to the selected relay")
	}
	return nil
}

func equalNetworkSelection(first, second *protocol.NetworkSelection) bool {
	if first == nil || second == nil {
		return first == second
	}
	left, leftErr := json.Marshal(first)
	right, rightErr := json.Marshal(second)
	return leftErr == nil && rightErr == nil && bytes.Equal(left, right)
}
