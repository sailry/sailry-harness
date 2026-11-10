package computer

import (
	"context"
	"encoding/json"
	"errors"
	"runtime"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const permissionDeadline = 15 * time.Second

// Probe reads the helper's own grants on the execution device. Diagnostics
// reuse the captured helper configuration of the MCP launch.
type Probe struct {
	goos string
	run  func(ctx context.Context, launch Invocation, request bool) ([]byte, error)
}

// NewProbe runs `agent-desktop permissions` on this platform.
func NewProbe() Probe { return Probe{goos: runtime.GOOS, run: diagnose} }

// Gated reports whether this platform gates the helper behind system grants.
func (probe Probe) Gated() bool { return probe.goos == "darwin" }

// Read reports the helper's grants. Only macOS gates them; other platforms
// report no requirement without starting the helper.
func (probe Probe) Read(ctx context.Context, node, caller protocol.NodeID, launch *Invocation) (protocol.ComputerPermissions, error) {
	value := protocol.ComputerPermissions{Node: node, Platform: platform(probe.goos), Local: node == caller}
	if probe.goos != "darwin" {
		return value, nil
	}
	if launch == nil {
		return value, Unconfigured()
	}
	body, err := probe.run(ctx, *launch, false)
	if err != nil {
		return value, err
	}
	value.ScreenCapture, value.Accessibility, err = parse(body)
	return value, err
}

// Request raises the helper's system prompts for a missing grant and reads
// the result from a fresh process, because Screen Recording state is fixed
// at process launch.
func (probe Probe) Request(ctx context.Context, node, caller protocol.NodeID, permission protocol.ComputerPermission, launch *Invocation) (protocol.ComputerPermissions, error) {
	if node != caller {
		return protocol.ComputerPermissions{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "system permissions must be granted on the execution device"}
	}
	if probe.goos != "darwin" {
		return protocol.ComputerPermissions{}, unavailable("computer permission setup is unavailable on this platform")
	}
	current, err := probe.Read(ctx, node, caller, launch)
	if err != nil {
		return current, err
	}
	granted := current.Accessibility
	if permission == protocol.ComputerPermissionScreenCapture {
		granted = current.ScreenCapture
	}
	if granted == nil || *granted {
		return current, nil
	}
	if _, err := probe.run(ctx, *launch, true); err != nil {
		return current, err
	}
	return probe.Read(ctx, node, caller, launch)
}

// Unconfigured reports a permission command without a captured helper.
func Unconfigured() error {
	return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "computer use requires a captured helper configuration"}
}

// platform names the operating system as the protocol's other engine does.
func platform(goos string) string {
	if goos == "darwin" {
		return "macos"
	}
	return goos
}

// parse reads the `agent-desktop permissions` envelope. A nil grant means
// not required or undetermined.
func parse(body []byte) (screen, accessibility *bool, err error) {
	type state struct {
		State string `json:"state"`
	}
	var envelope struct {
		OK      bool   `json:"ok"`
		Command string `json:"command"`
		Data    *struct {
			Accessibility   *state `json:"accessibility"`
			ScreenRecording *state `json:"screen_recording"`
		} `json:"data"`
	}
	malformed := unavailable("computer permission diagnostics are malformed")
	if json.Unmarshal(body, &envelope) != nil || !envelope.OK || envelope.Command != "permissions" ||
		envelope.Data == nil || envelope.Data.Accessibility == nil || envelope.Data.ScreenRecording == nil {
		return nil, nil, malformed
	}
	grant := func(value string) (*bool, error) {
		switch value {
		case "granted", "denied":
			granted := value == "granted"
			return &granted, nil
		case "not_required", "unknown":
			return nil, nil
		}
		return nil, malformed
	}
	if screen, err = grant(envelope.Data.ScreenRecording.State); err != nil {
		return nil, nil, err
	}
	if accessibility, err = grant(envelope.Data.Accessibility.State); err != nil {
		return nil, nil, err
	}
	return screen, accessibility, nil
}

func diagnose(ctx context.Context, launch Invocation, request bool) ([]byte, error) {
	cancelled := &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "computer permission check was cancelled"}
	if ctx.Err() != nil {
		return nil, cancelled
	}
	args := []string{"permissions"}
	if request {
		args = append(args, "--request")
	}
	bounded, cancel := context.WithTimeout(ctx, permissionDeadline)
	defer cancel()
	body, succeeded, err := execute(bounded, launch, args, false)
	switch {
	case ctx.Err() != nil:
		return nil, cancelled
	case errors.Is(err, context.DeadlineExceeded):
		return nil, unavailable("computer permission diagnostics timed out")
	case errors.Is(err, errOutputLimit):
		return nil, unavailable("computer permission diagnostics exceed the output limit")
	case err != nil || !succeeded:
		return nil, unavailable("computer permission diagnostics failed")
	}
	return body, nil
}
