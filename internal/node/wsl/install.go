package wsl

import (
	"context"
	_ "embed"
	"errors"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/mod/semver"
)

//go:embed preflight.sh
var preflightSource string

//go:embed release.sh
var releaseSource string

const ticketScript = "cat \"$HOME/.sailry-host/bootstrap.ticket\" 2>/dev/null || test ! -e \"$HOME/.sailry-host/bootstrap.ticket\"\n"

// ConfigureNetwork applies and confirms the selected network on a paired
// Host. It is the Host deployment's implementation shared with SSH.
type ConfigureNetwork func(ctx context.Context, endpoint *link.Link, address link.EndpointAddress, request protocol.RequestID, selection protocol.NetworkSelection) error

// Execute installs the current Host release into the distribution and pairs
// it with the local endpoint. local is the controller's own Node identity.
func (installation *Installation) Execute(environment []string, endpoint *link.Link, local protocol.NodeID, network *protocol.NetworkSelection, configure ConfigureNetwork) (protocol.HostInstallationOutcome, error) {
	ctx := installation.ctx
	installation.stage("detecting")
	found, err := discover(ctx, environment)
	if err != nil {
		return protocol.HostInstallationOutcome{}, err
	}
	if err := deployable(found.distributions, installation.distribution); err != nil {
		return protocol.HostInstallationOutcome{}, err
	}
	arguments := launchArguments(installation.distribution)
	preflight := normalize(preflightSource)
	if _, err := run(ctx, found.executable, arguments, environment, preflight, 30*time.Second); err != nil {
		return protocol.HostInstallationOutcome{}, err
	}
	if ctx.Err() != nil {
		return protocol.HostInstallationOutcome{}, &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "WSL Host installation was cancelled"}
	}
	version := link.ProductVersion
	release, err := ReleaseScript(version, false, network)
	if err != nil {
		return protocol.HostInstallationOutcome{}, err
	}
	installation.stage("installing")
	if _, err := run(ctx, found.executable, arguments, environment, preflight+"\n"+release, 480*time.Second); err != nil {
		var fault *protocol.Fault
		if errors.As(err, &fault) && fault.Code == protocol.ErrorCodeCancelled {
			err = unknown("Host installation was cancelled after launch; inspect the distribution before retrying")
		}
		return protocol.HostInstallationOutcome{}, err
	}
	installation.stage("pairing")
	pairing, stop := context.WithTimeout(ctx, 120*time.Second)
	defer stop()
	outcome, err := installation.pair(pairing, found.executable, arguments, environment, endpoint, local, network, version, configure)
	if ctx.Err() != nil {
		return protocol.HostInstallationOutcome{}, unknown("Host installed but pairing was cancelled; inspect the distribution before retrying")
	}
	if pairing.Err() != nil {
		return protocol.HostInstallationOutcome{}, unknown("Host installed but pairing completion is unconfirmed")
	}
	return outcome, err
}

// pairingAttempts is how many times an installation tries to pair with the
// Host it installed.
const pairingAttempts = 5

// pairBootstrap is the retry of pair, apart from the process and the endpoint
// it reads and pairs through. wait reports whether time remains for another
// read.
func pairBootstrap(read func() (string, error), pair func(string) (link.EndpointAddress, error), wait func() (bool, error)) (link.EndpointAddress, error) {
	for failures := 0; ; {
		ticket, err := read()
		if err != nil {
			return link.EndpointAddress{}, unknown("Host installed but its bootstrap ticket could not be read")
		}
		if ticket != "" {
			address, err := pair(ticket)
			if err == nil {
				return address, nil
			}
			var settled *protocol.Fault
			if failures++; failures == pairingAttempts || errors.As(err, &settled) && settled.Code == protocol.ErrorCodeOutcomeUnknown {
				return link.EndpointAddress{}, unknown("Host installed but pairing could not be confirmed")
			}
		}
		more, err := wait()
		if err != nil {
			return link.EndpointAddress{}, err
		}
		if !more {
			if failures > 0 {
				return link.EndpointAddress{}, unknown("Host installed but pairing could not be confirmed")
			}
			return link.EndpointAddress{}, unknown("Host service started but pairing is unavailable; inspect the distribution service")
		}
	}
}

// pair waits for the installed service to publish its bootstrap ticket, then
// confirms an independent endpoint running the expected release. The service
// has only just started, so one refused or unreachable attempt does not end
// the installation: the ticket is read again, because the Host replaces one
// that expired, and pairing is tried up to pairingAttempts times within 90
// seconds. A result that may have changed trust is not retried.
func (installation *Installation) pair(ctx context.Context, executable string, arguments, environment []string, endpoint *link.Link, local protocol.NodeID, network *protocol.NetworkSelection, version string, configure ConfigureNetwork) (protocol.HostInstallationOutcome, error) {
	deadline := time.Now().Add(90 * time.Second)
	read := func() (string, error) {
		output, err := run(ctx, executable, arguments, environment, ticketScript, 5*time.Second)
		return strings.TrimSpace(output), err
	}
	wait := func() (bool, error) {
		if !time.Now().Before(deadline) {
			return false, nil
		}
		timer := time.NewTimer(time.Second)
		defer timer.Stop()
		select {
		case <-ctx.Done():
			return false, ctx.Err()
		case <-timer.C:
			return true, nil
		}
	}
	address, err := pairBootstrap(read, func(ticket string) (link.EndpointAddress, error) { return endpoint.Pair(ctx, ticket) }, wait)
	if err != nil {
		return protocol.HostInstallationOutcome{}, err
	}
	if address.ID == local {
		return protocol.HostInstallationOutcome{}, unknown("Installed Host identity is not independent")
	}
	info, err := endpoint.Inspect(ctx, address)
	if err != nil {
		return protocol.HostInstallationOutcome{}, unknown("Host installed but its endpoint could not be confirmed")
	}
	current := info.SoftwareVersion != nil && semver.IsValid("v"+*info.SoftwareVersion) && semver.IsValid("v"+version) && semver.Compare("v"+*info.SoftwareVersion, "v"+version) == 0
	if !info.Execution || !current {
		return protocol.HostInstallationOutcome{}, unknown("Installed endpoint does not provide the expected Host release")
	}
	if network != nil && configure(ctx, endpoint, address, installation.key.request, *network) != nil {
		return protocol.HostInstallationOutcome{Kind: "network_unconfirmed", Data: protocol.HostInstallationOutcomeNetworkUnconfirmedPayload{Node: address.ID, Selection: *network}}, nil
	}
	return protocol.HostInstallationOutcome{Kind: "installed", Data: protocol.HostInstallationOutcomeInstalledPayload{Node: address.ID}}, nil
}

// launchArguments passes the distribution as one argument and reads the
// script from standard input, so no name or script is ever shell-quoted.
func launchArguments(distribution string) []string {
	return []string{"--distribution", distribution, "--cd", "~", "--exec", "/bin/sh", "-s"}
}

// ReleaseScript prefixes the official release bootstrap with its version,
// whether it updates an installed Host, and the network arguments forwarded
// to the Host installer. SSH Add Host runs the same script.
func ReleaseScript(version string, update bool, network *protocol.NetworkSelection) (string, error) {
	arguments := []string{}
	if network != nil {
		switch network.Kind {
		case "iroh":
			arguments = append(arguments, "'--internet'")
		case "custom":
			var relays []string
			switch payload := network.Data.(type) {
			case protocol.NetworkSelectionCustomPayload:
				relays = payload.Relays
			case *protocol.NetworkSelectionCustomPayload:
				if payload != nil {
					relays = payload.Relays
				}
			}
			for _, relay := range relays {
				if strings.ContainsRune(relay, '\x00') {
					return "", invalid("relay URL contains a null byte")
				}
				arguments = append(arguments, "'--relay'", "'"+strings.ReplaceAll(relay, "'", "'\"'\"'")+"'")
			}
		default:
			return "", invalid("invalid Host network selection")
		}
	}
	mode := "0"
	if update {
		mode = "1"
	}
	return "install_version='" + version + "'\ninstall_update=" + mode + "\nset -- " + strings.Join(arguments, " ") + "\n" + normalize(releaseSource), nil
}

// normalize undoes checkout line-ending conversion of an embedded script and
// keeps any lone carriage return inside its text.
func normalize(source string) string {
	return strings.ReplaceAll(source, "\r\n", "\n")
}
