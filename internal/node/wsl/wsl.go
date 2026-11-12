// Package wsl owns installed WSL distribution discovery and independent Linux
// Host deployment into a WSL2 distribution for the execution Node.
package wsl

import (
	"context"
	"strings"
	"sync"
	"unicode"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type installKey struct {
	caller  protocol.NodeID
	request protocol.RequestID
}

type active struct {
	distribution string
	progress     protocol.SshInstallProgress
	cancel       context.CancelFunc
}

// Service tracks active WSL Host installations for progress and cancellation.
type Service struct {
	mu     sync.Mutex
	active map[installKey]*active
}

// Installation is one registered installation. Close releases its registration.
type Installation struct {
	service      *Service
	key          installKey
	distribution string
	ctx          context.Context
	cancel       context.CancelFunc
}

// source is the resolved wsl.exe launcher and the distributions it reports.
type source struct {
	executable    string
	distributions []protocol.WslDistribution
}

func NewService() *Service {
	return &Service{active: map[installKey]*active{}}
}

// List reports the registered distributions and their WSL versions.
func (service *Service) List(ctx context.Context, environment []string) ([]protocol.WslDistribution, error) {
	found, err := discover(ctx, environment)
	if err != nil {
		return nil, err
	}
	// An empty result stays a list on the wire.
	return append([]protocol.WslDistribution{}, found.distributions...), nil
}

// Begin registers an install_wsl_host request. The context bounds the whole
// installation; cancelling it or calling Cancel stops the active step.
func (service *Service) Begin(ctx context.Context, caller protocol.NodeID, request protocol.Request) (*Installation, error) {
	input, ok := request.Command.Data.(*protocol.CommandInstallWslHostPayload)
	if !ok {
		return nil, invalid("WSL installation command expected")
	}
	if err := validate(input.Distribution); err != nil {
		return nil, err
	}
	service.mu.Lock()
	defer service.mu.Unlock()
	for _, entry := range service.active {
		if asciiLower(entry.distribution) == asciiLower(input.Distribution) {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "Host installation is already active in this distribution"}
		}
	}
	scoped, cancel := context.WithCancel(ctx)
	key := installKey{caller, request.ID}
	service.active[key] = &active{
		distribution: input.Distribution,
		progress:     protocol.SshInstallProgress{Kind: "connecting"},
		cancel:       cancel,
	}
	return &Installation{service: service, key: key, distribution: input.Distribution, ctx: scoped, cancel: cancel}, nil
}

// Progress reads the current stage of the caller's own active installation.
func (service *Service) Progress(caller protocol.NodeID, request protocol.RequestID) (protocol.SshInstallProgress, error) {
	service.mu.Lock()
	defer service.mu.Unlock()
	entry := service.active[installKey{caller, request}]
	if entry == nil {
		return protocol.SshInstallProgress{}, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "WSL Host installation is not active"}
	}
	return entry.progress, nil
}

// Cancel stops the caller's own active installation. A request that is not
// active is already settled, so cancelling it succeeds without effect.
func (service *Service) Cancel(caller protocol.NodeID, request protocol.RequestID) error {
	service.mu.Lock()
	defer service.mu.Unlock()
	entry := service.active[installKey{caller, request}]
	if entry == nil {
		return nil
	}
	entry.cancel()
	return nil
}

func (installation *Installation) Close() {
	installation.cancel()
	installation.service.mu.Lock()
	defer installation.service.mu.Unlock()
	delete(installation.service.active, installation.key)
}

func (installation *Installation) stage(kind string) {
	installation.service.mu.Lock()
	defer installation.service.mu.Unlock()
	if entry := installation.service.active[installation.key]; entry != nil {
		entry.progress = protocol.SshInstallProgress{Kind: kind}
	}
}

// validate accepts any registered name that wsl.exe can receive as one
// argument without being read as an option.
func validate(distribution string) error {
	if distribution == "" || len(distribution) > 256 || strings.HasPrefix(distribution, "-") || strings.ContainsFunc(distribution, unicode.IsControl) {
		return invalid("invalid WSL distribution name")
	}
	return nil
}

// deployable requires the exact registered name and a WSL2 distribution.
func deployable(distributions []protocol.WslDistribution, name string) error {
	for _, entry := range distributions {
		if entry.Name != name {
			continue
		}
		if entry.Version != 2 {
			return unavailable("Host requires a WSL2 distribution")
		}
		return nil
	}
	return &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "WSL distribution is no longer installed"}
}

func asciiLower(value string) string {
	return strings.Map(func(char rune) rune {
		if char >= 'A' && char <= 'Z' {
			return char + 'a' - 'A'
		}
		return char
	}, value)
}

func invalid(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}

func unavailable(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: message}
}

func unknown(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: message}
}
