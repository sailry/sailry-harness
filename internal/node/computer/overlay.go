package computer

import (
	"bytes"
	"context"
	_ "embed"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"time"
)

const (
	overlayDeadline = 10 * time.Second
	// SessionVariable names the overlay session a helper launch joins.
	SessionVariable = "AGENT_DESKTOP_SESSION"
	homeVariable    = "AGENT_DESKTOP_HOME"
)

// agent-desktop v1.2.0's fresh session manifest supports custom PNG cursors on
// macOS and Windows; its X11 renderer only draws the built-in arrow.
//
//go:embed cursor.png
var cursor []byte

// Overlay owns one cursor-overlay session per helper home. The overlay never
// moves the physical pointer. A failed step leaves the launch headless.
type Overlay struct {
	once     sync.Once
	lock     chan struct{}
	stop     context.Context
	shutdown context.CancelFunc
	homes    map[string]*overlayHome
	// run executes one helper session command and returns its response line
	// and whether the helper exited successfully.
	run func(ctx context.Context, invocation Invocation, args []string) ([]byte, bool, error)
}

type overlayHome struct {
	pack       string
	invocation Invocation
	session    string
	ready      bool
	collected  bool
}

func (overlay *Overlay) init() {
	overlay.once.Do(func() {
		overlay.lock = make(chan struct{}, 1)
		overlay.stop, overlay.shutdown = context.WithCancel(context.Background())
		overlay.homes = map[string]*overlayHome{}
		if overlay.run == nil {
			overlay.run = func(ctx context.Context, invocation Invocation, args []string) ([]byte, bool, error) {
				return execute(ctx, invocation, args, true)
			}
		}
	})
}

// Attach returns the session an allowed package's helper launch joins. It
// keeps a session the launch already declares and starts none for a launch
// without a helper home.
func (overlay *Overlay) Attach(pack string, allowed bool, launch Invocation) (string, bool) {
	overlay.init()
	if _, declared := launch.Env[SessionVariable]; declared {
		return "", false
	}
	path, found := launch.Env[homeVariable]
	if !found {
		return "", false
	}
	timer := time.NewTimer(overlayDeadline)
	defer timer.Stop()
	select {
	case overlay.lock <- struct{}{}:
	case <-timer.C:
		report("lock", "TIMEOUT")
		return "", false
	}
	defer func() { <-overlay.lock }()
	if overlay.stop.Err() != nil {
		return "", false
	}
	home := overlay.homes[path]
	if !allowed {
		if home != nil {
			overlay.end(home)
		}
		return "", false
	}
	if home == nil {
		home = &overlayHome{pack: pack}
		overlay.homes[path] = home
	}
	home.invocation = launch
	if home.session != "" && !home.ready {
		overlay.end(home)
		if home.session != "" {
			return "", false
		}
	}
	if home.session != "" && active(path, home.session) {
		return home.session, true
	}
	home.session, home.ready = "", false
	if !home.collected {
		home.collected = true
		if _, code := overlay.command(home, "session", "gc", "--ended"); code != "" {
			report("gc", code)
		}
	}
	if overlay.stop.Err() != nil {
		return "", false
	}
	data, code := overlay.command(home, "session", "start", "--no-trace", "--name", "sailry")
	session := ""
	if code == "" {
		session, code = sessionID(data)
	}
	if code != "" {
		report("start", code)
		if code == "TIMEOUT" || code == "CANCELLED" {
			overlay.sweep(home)
		}
		return "", false
	}
	home.session = session
	if code := configure(path, session); code != "" {
		report("configure", code)
		overlay.end(home)
		return "", false
	}
	home.ready = true
	return session, true
}

// Tracks reports whether a package has an overlay session.
func (overlay *Overlay) Tracks(pack string) bool {
	overlay.init()
	overlay.lock <- struct{}{}
	defer func() { <-overlay.lock }()
	for _, home := range overlay.homes {
		if home.pack == pack && home.session != "" {
			return true
		}
	}
	return false
}

// Retain ends the sessions of packages that no longer allow the overlay:
// those of one package, or of every package when pack is empty.
func (overlay *Overlay) Retain(pack string, allowed []string) {
	overlay.init()
	overlay.lock <- struct{}{}
	defer func() { <-overlay.lock }()
	for _, home := range overlay.homes {
		if pack != "" && pack != home.pack {
			continue
		}
		kept := false
		for _, name := range allowed {
			kept = kept || name == home.pack
		}
		if !kept {
			overlay.end(home)
		}
	}
}

// Shutdown interrupts a pending start and ends every session.
func (overlay *Overlay) Shutdown() {
	overlay.init()
	overlay.shutdown()
	overlay.lock <- struct{}{}
	defer func() { <-overlay.lock }()
	var ending sync.WaitGroup
	for _, home := range overlay.homes {
		ending.Go(func() { overlay.end(home) })
	}
	ending.Wait()
}

func (overlay *Overlay) end(home *overlayHome) {
	if home.session == "" {
		return
	}
	if _, code := overlay.command(home, "session", "end", home.session); code != "" {
		report("end", code)
		return
	}
	home.session, home.ready = "", false
}

// sweep ends this owner's unrecorded sessions after an interrupted start.
func (overlay *Overlay) sweep(home *overlayHome) {
	data, code := overlay.command(home, "session", "list")
	if code != "" {
		report("list", code)
		return
	}
	var listed struct {
		Sessions []map[string]json.RawMessage `json:"sessions"`
	}
	_ = json.Unmarshal(data, &listed)
	var ending sync.WaitGroup
	for _, entry := range listed.Sessions {
		var name string
		if json.Unmarshal(entry["name"], &name) != nil || name != "sailry" || !null(entry["ended_at"]) {
			continue
		}
		body, _ := json.Marshal(entry)
		id, code := sessionID(body)
		if code != "" || id == home.session {
			continue
		}
		ending.Go(func() {
			if _, code := overlay.command(home, "session", "end", id); code != "" {
				report("orphan_end", code)
			}
		})
	}
	ending.Wait()
}

// command runs one session command under its own deadline. Only a start or a
// collection is interrupted by shutdown, so cleanup still runs.
func (overlay *Overlay) command(home *overlayHome, args ...string) (json.RawMessage, string) {
	parent := context.Background()
	if args[1] == "start" || args[1] == "gc" {
		parent = overlay.stop
	}
	ctx, cancel := context.WithTimeout(parent, overlayDeadline)
	defer cancel()
	body, succeeded, err := overlay.run(ctx, home.invocation.without(SessionVariable), args)
	switch {
	case errors.Is(err, context.DeadlineExceeded):
		return nil, "TIMEOUT"
	case errors.Is(err, context.Canceled):
		return nil, "CANCELLED"
	case errors.Is(err, errOutputLimit):
		return nil, "OUTPUT_LIMIT"
	case errors.Is(err, errSpawn):
		return nil, "SPAWN"
	case err != nil:
		return nil, "OUTPUT"
	}
	var envelope struct {
		OK    bool            `json:"ok"`
		Data  json.RawMessage `json:"data"`
		Error *struct {
			Code string `json:"code"`
		} `json:"error"`
	}
	if json.Unmarshal(body, &envelope) != nil {
		return nil, "INVALID_RESPONSE"
	}
	if succeeded && envelope.OK {
		return envelope.Data, ""
	}
	if envelope.Error != nil && failureCode(envelope.Error.Code) {
		return nil, envelope.Error.Code
	}
	return nil, "FAILED"
}

func failureCode(code string) bool {
	if code == "" || len(code) > 64 {
		return false
	}
	for _, char := range []byte(code) {
		if !(char >= 'A' && char <= 'Z' || char >= '0' && char <= '9' || char == '_') {
			return false
		}
	}
	return true
}

func sessionID(data []byte) (string, string) {
	var value struct {
		ID string `json:"session_id"`
	}
	if json.Unmarshal(data, &value) != nil || value.ID == "" || len(value.ID) > 64 {
		return "", "INVALID_SESSION"
	}
	for _, char := range []byte(value.ID) {
		if !(char >= 'a' && char <= 'z' || char >= 'A' && char <= 'Z' || char >= '0' && char <= '9' || char == '-' || char == '_') {
			return "", "INVALID_SESSION"
		}
	}
	return value.ID, ""
}

func null(value json.RawMessage) bool {
	return len(value) == 0 || bytes.Equal(bytes.TrimSpace(value), []byte("null"))
}

// manifest reads a live session's bounded manifest.
func manifest(home, session string) map[string]json.RawMessage {
	file, err := os.Open(filepath.Join(home, "sessions", session, "session.json"))
	if err != nil {
		return nil
	}
	defer file.Close()
	body, err := io.ReadAll(io.LimitReader(file, maxOutput+1))
	if err != nil || len(body) > maxOutput {
		return nil
	}
	var value map[string]json.RawMessage
	var id string
	if json.Unmarshal(body, &value) != nil || json.Unmarshal(value["id"], &id) != nil || id != session || !null(value["ended_at"]) {
		return nil
	}
	return value
}

func active(home, session string) bool { return manifest(home, session) != nil }

// configure enables the overlay in the session manifest before MCP starts, so
// the first control draws without the helper's greeting.
func configure(home, session string) string {
	value := manifest(home, session)
	if value == nil {
		return "INVALID_SESSION"
	}
	directory := filepath.Join(home, "sessions", session)
	style := map[string]any{"size": 0.75, "rim": "#FFFFFF"}
	if runtime.GOOS == "darwin" || runtime.GOOS == "windows" {
		image := filepath.Join(directory, "cursor.png")
		if os.WriteFile(image, cursor, 0o600) != nil {
			return "CURSOR_WRITE"
		}
		style = map[string]any{"accent": "#4299FF", "images": map[string]any{
			"arrow": map[string]any{"path": image, "hotspot": map[string]int{"x": 16, "y": 16}},
		}}
	}
	overlay, err := json.Marshal(map[string]any{"enabled": true, "style": style})
	if err != nil {
		return "INVALID_SESSION"
	}
	value["cursor_overlay"] = overlay
	content, err := json.Marshal(value)
	if err != nil {
		return "INVALID_SESSION"
	}
	temporary, err := os.CreateTemp(directory, ".session-*")
	if err != nil {
		return "CONFIG_WRITE"
	}
	_, err = temporary.Write(content)
	if err = errors.Join(err, temporary.Close()); err == nil {
		err = os.Rename(temporary.Name(), filepath.Join(directory, "session.json"))
	}
	if err != nil {
		_ = os.Remove(temporary.Name())
		return "CONFIG_WRITE"
	}
	return ""
}

func report(operation, code string) {
	slog.Warn("computer overlay step failed", "operation", operation, "code", code)
}
