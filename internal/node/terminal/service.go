package terminal

import (
	"context"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"

	myterminal "github.com/egoist/mygo/plugins/terminal"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type Launch struct {
	ID          protocol.TerminalID
	Caller      protocol.NodeID
	Worktree    protocol.WorktreeID
	Root        string
	Tool        *protocol.TerminalToolsTool
	Access      protocol.ProcessAccess
	Command     []string
	Environment []string
	// Settings are the saved terminal settings: their environment applies to
	// every local launch and their shell to one without a tool or command.
	Settings protocol.TerminalSettings
	// SSH and Connection replace the local program with a remote shell. The
	// terminal then has no worktree, access policy or directory.
	SSH        *protocol.SshID
	Connection *SSHConnection
	Viewport   protocol.TerminalViewport
	Appearance protocol.TerminalAppearance
	Revision   uint64
}

type Service struct {
	ctx     context.Context
	node    protocol.NodeID
	changed func(protocol.TerminalInfo)
	mu      sync.Mutex
	stopped bool
	handles map[protocol.TerminalID]*handle
	starts  map[protocol.TerminalID]*starting
	work    sync.WaitGroup
	done    chan struct{}
}

type starting struct {
	done chan struct{}
	info protocol.TerminalInfo
	err  error
}

type handle struct {
	service  *Service
	mu       sync.Mutex
	terminal *myterminal.Terminal
	info     protocol.TerminalInfo
	viewport protocol.TerminalViewport
}

func New(ctx context.Context, node protocol.NodeID, changed func(protocol.TerminalInfo)) *Service {
	service := &Service{ctx: ctx, node: node, changed: changed, handles: map[protocol.TerminalID]*handle{}, starts: map[protocol.TerminalID]*starting{}, done: make(chan struct{})}
	context.AfterFunc(ctx, func() { _ = service.Close() })
	return service
}

func (service *Service) Start(ctx context.Context, launch Launch) (protocol.TerminalInfo, error) {
	if err := ctx.Err(); err != nil {
		return protocol.TerminalInfo{}, err
	}
	if err := ValidateViewport(launch.Viewport); err != nil {
		return protocol.TerminalInfo{}, err
	}
	if launch.Connection == nil && !filepath.IsAbs(launch.Root) {
		return protocol.TerminalInfo{}, fault(protocol.ErrorCodeInvalidRequest, "terminal worktree root must be absolute")
	}
	if launch.ID == (protocol.TerminalID{}) {
		launch.ID = protocol.NewTerminalID()
	}
	if launch.Revision == 0 {
		launch.Revision = 1
	}
	command, directory := slices.Clone(launch.Command), launch.Root
	environment := Environment(launch.Environment, launch.Settings)
	switch {
	case launch.Connection != nil || len(command) != 0:
	case launch.Tool != nil:
		path, err := resolveTool(launch.Root, environment, *launch.Tool)
		if err != nil {
			return protocol.TerminalInfo{}, err
		}
		command = []string{path}
	default:
		var variables []string
		var err error
		if command, variables, directory, err = shellCommand(runtime.GOOS, launch.Settings, launch.Root); err != nil {
			return protocol.TerminalInfo{}, err
		}
		environment = append(environment, variables...)
	}
	service.mu.Lock()
	if service.stopped {
		service.mu.Unlock()
		return protocol.TerminalInfo{}, fault(protocol.ErrorCodeUnavailable, "terminal service is stopped")
	}
	if current := service.handles[launch.ID]; current != nil {
		current.mu.Lock()
		info := current.info
		current.mu.Unlock()
		service.mu.Unlock()
		return info, nil
	}
	if pending := service.starts[launch.ID]; pending != nil {
		service.mu.Unlock()
		select {
		case <-ctx.Done():
			return protocol.TerminalInfo{}, ctx.Err()
		case <-pending.done:
			return pending.info, pending.err
		}
	}
	running := len(service.starts)
	for _, current := range service.handles {
		current.mu.Lock()
		if current.info.Status.Kind == "running" {
			running++
		}
		current.mu.Unlock()
	}
	if running >= 32 {
		service.mu.Unlock()
		return protocol.TerminalInfo{}, fault(protocol.ErrorCodeBusy, "terminal capacity is exhausted")
	}
	pending := &starting{done: make(chan struct{})}
	service.starts[launch.ID] = pending
	service.work.Add(1)
	service.mu.Unlock()
	defer service.work.Done()
	finish := func(info protocol.TerminalInfo, err error) {
		service.mu.Lock()
		pending.info, pending.err = info, err
		delete(service.starts, launch.ID)
		close(pending.done)
		service.mu.Unlock()
	}
	title := ""
	if launch.Tool != nil {
		title = string(*launch.Tool)
	}
	access := launch.Access
	if access == "" {
		access = protocol.ProcessAccessUnrestricted
	}
	// The terminal plugin owns the pseudo-terminal, so its device cannot be
	// granted to a sandbox profile here: a workspace terminal needs a command
	// its caller already confined. A platform without the sandbox runs as is.
	if launch.Connection == nil && access == protocol.ProcessAccessWorkspace && len(launch.Command) == 0 && process.SandboxSupported() {
		err := fault(protocol.ErrorCodeNotConfigured, "workspace terminal requires a prepared sandbox command")
		finish(protocol.TerminalInfo{}, err)
		return protocol.TerminalInfo{}, err
	}
	value := &handle{service: service, viewport: launch.Viewport, info: protocol.TerminalInfo{
		ID: launch.ID, Worktree: &launch.Worktree, Tool: launch.Tool, Access: &access,
		Status: protocol.TerminalStatus{Kind: "running"}, Title: &title, Directory: &launch.Root,
		Owner: &launch.Caller, Revision: launch.Revision,
	}}
	options := myterminal.Options{
		Command: command, Dir: directory, Env: environment,
		// Prompt marks belong to the default startup of a plain shell.
		ShellIntegration: launch.Tool == nil && len(launch.Settings.ShellArguments) == 0,
	}
	if launch.Connection != nil {
		value.info.Worktree, value.info.Access, value.info.Directory, value.info.Ssh = nil, nil, nil, launch.SSH
		options = myterminal.Options{Conn: launch.Connection}
	}
	options.Theme = terminalTheme(launch.Appearance)
	options.ClipboardRead = myterminal.ClipboardForward
	options.OnTitle = func(title string) {
		value.mu.Lock()
		if value.info.Status.Kind != "running" {
			value.mu.Unlock()
			return
		}
		value.info.Title = &title
		info := value.info
		value.mu.Unlock()
		service.publish(info)
	}
	options.OnExit = func(code int) {
		value.mu.Lock()
		if value.info.Status.Kind != "running" {
			value.mu.Unlock()
			return
		}
		switch {
		case launch.Connection != nil:
			// A connection always reports code zero; the session has the status.
			value.info.Status = launch.Connection.status()
		case code < 0:
			value.info.Status = protocol.TerminalStatus{Kind: "failed", Data: protocol.TerminalStatusFailedPayload{Message: "terminal process could not start"}}
		default:
			value.info.Status = protocol.TerminalStatus{Kind: "exited", Data: protocol.TerminalStatusExitedPayload{Code: uint32(code)}}
		}
		value.info.Owner = nil
		value.info.Revision++
		info := value.info
		value.mu.Unlock()
		service.publish(info)
	}
	terminal, err := myterminal.New(options)
	if err != nil {
		finish(protocol.TerminalInfo{}, err)
		return protocol.TerminalInfo{}, err
	}
	value.terminal = terminal
	terminal.ResizePixels(int(launch.Viewport.Columns), int(launch.Viewport.Rows), cellPixels(launch.Viewport.PixelWidth, launch.Viewport.Columns), cellPixels(launch.Viewport.PixelHeight, launch.Viewport.Rows))
	service.mu.Lock()
	if service.stopped || ctx.Err() != nil {
		service.mu.Unlock()
		value.mu.Lock()
		value.info.Status = protocol.TerminalStatus{Kind: "stopped"}
		value.info.Owner = nil
		value.mu.Unlock()
		_ = terminal.Close()
		err := ctx.Err()
		if err == nil {
			err = fault(protocol.ErrorCodeUnavailable, "terminal service is stopped")
		}
		finish(protocol.TerminalInfo{}, err)
		return protocol.TerminalInfo{}, err
	}
	service.handles[launch.ID] = value
	value.mu.Lock()
	info := value.info
	value.mu.Unlock()
	pending.info = info
	delete(service.starts, launch.ID)
	close(pending.done)
	service.mu.Unlock()
	service.publish(info)
	return info, nil
}

func (service *Service) publish(info protocol.TerminalInfo) {
	if service.changed != nil {
		service.changed(info)
	}
}

func (service *Service) get(id protocol.TerminalID) (*handle, error) {
	service.mu.Lock()
	defer service.mu.Unlock()
	if service.stopped {
		return nil, fault(protocol.ErrorCodeUnavailable, "terminal service is stopped")
	}
	value := service.handles[id]
	if value == nil {
		return nil, fault(protocol.ErrorCodeNotFound, "terminal is not running on this Node")
	}
	return value, nil
}

func (service *Service) Info(id protocol.TerminalID) (protocol.TerminalInfo, error) {
	value, err := service.get(id)
	if err != nil {
		return protocol.TerminalInfo{}, err
	}
	value.mu.Lock()
	defer value.mu.Unlock()
	return value.info, nil
}

func (service *Service) List(worktree protocol.WorktreeID) []protocol.TerminalInfo {
	service.mu.Lock()
	defer service.mu.Unlock()
	values := []protocol.TerminalInfo{}
	for _, value := range service.handles {
		value.mu.Lock()
		if value.info.Worktree != nil && (*value.info.Worktree == worktree || worktree == (protocol.WorktreeID{})) {
			values = append(values, value.info)
		}
		value.mu.Unlock()
	}
	slices.SortFunc(values, func(first, second protocol.TerminalInfo) int {
		return strings.Compare(first.ID.String(), second.ID.String())
	})
	return values
}

func (service *Service) Active(worktree protocol.WorktreeID) bool {
	for _, info := range service.List(worktree) {
		if info.Status.Kind == "running" {
			return true
		}
	}
	return false
}

func (service *Service) Close() error {
	service.mu.Lock()
	if service.stopped {
		service.mu.Unlock()
		<-service.done
		return nil
	}
	service.stopped = true
	handles := service.handles
	service.handles = map[protocol.TerminalID]*handle{}
	service.mu.Unlock()
	for _, value := range handles {
		value.mu.Lock()
		value.info.Status = protocol.TerminalStatus{Kind: "stopped"}
		value.info.Owner = nil
		value.mu.Unlock()
		_ = value.terminal.Close()
	}
	service.work.Wait()
	close(service.done)
	return nil
}

func terminalTheme(appearance protocol.TerminalAppearance) *myterminal.Theme {
	color := func(value protocol.TerminalRgb) ui.Color { return ui.RGB(value.Red, value.Green, value.Blue) }
	theme := &myterminal.Theme{Foreground: color(appearance.Foreground), Background: color(appearance.Background)}
	for index, entry := range appearance.Palette {
		theme.Palette[index] = color(entry)
	}
	return theme
}

func cellPixels(pixels uint32, cells uint16) int {
	if cells == 0 {
		return 0
	}
	return int(pixels / uint32(cells))
}

func fault(code protocol.ErrorCode, message string) error {
	return &protocol.Fault{Code: code, Message: message}
}

func (service *Service) Tools(root string, environment []string) []protocol.TerminalToolsToolInfo {
	tools := []protocol.TerminalToolsTool{protocol.TerminalToolsToolCodex, protocol.TerminalToolsToolClaude, protocol.TerminalToolsToolGemini, protocol.TerminalToolsToolAgy, protocol.TerminalToolsToolGrok, protocol.TerminalToolsToolOpencode, protocol.TerminalToolsToolKimi}
	values := make([]protocol.TerminalToolsToolInfo, 0, len(tools))
	for _, tool := range tools {
		_, err := resolveTool(root, environment, tool)
		values = append(values, protocol.TerminalToolsToolInfo{Tool: tool, Available: err == nil})
	}
	return values
}

func resolveTool(root string, environment []string, tool protocol.TerminalToolsTool) (string, error) {
	if _, err := tool.MarshalJSON(); err != nil {
		return "", fault(protocol.ErrorCodeInvalidRequest, "unknown terminal tool")
	}
	if path, found := findExecutable(root, environment, string(tool)); found {
		return path, nil
	}
	return "", fault(protocol.ErrorCodeNotConfigured, "terminal tool is unavailable on this Node")
}
