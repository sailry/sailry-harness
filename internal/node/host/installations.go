package host

import (
	"context"
	"encoding/json"
	"sync"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type installKey struct {
	caller  protocol.NodeID
	request protocol.RequestID
}

type installation struct {
	progress protocol.SshInstallProgress
	plugin   string
	cancel   context.CancelFunc
}

type DeploymentOptions struct {
	ProgramRoot string
	Profile     string
	ServicePath string
	CommandPath string
}

type Installations struct {
	mu      sync.Mutex
	active  map[installKey]*installation
	options DeploymentOptions
}

func NewInstallations(options DeploymentOptions) *Installations {
	return &Installations{active: map[installKey]*installation{}, options: options}
}

func pluginScope(plugin *protocol.PluginContext) string {
	if plugin == nil {
		return ""
	}
	body, _ := json.Marshal(plugin)
	return string(body)
}

func (owner *Installations) Begin(ctx context.Context, caller protocol.NodeID, request protocol.Request) (context.Context, func(), error) {
	owner.mu.Lock()
	defer owner.mu.Unlock()
	key := installKey{caller, request.ID}
	if _, exists := owner.active[key]; exists {
		return nil, nil, &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "Host installation is already active"}
	}
	scoped, cancel := context.WithCancel(ctx)
	stage := ""
	if request.Command.Kind == "install_host" || request.Command.Kind == "update_ssh_host" {
		stage = "connecting"
	}
	owner.active[key] = &installation{progress: protocol.SshInstallProgress{Kind: stage}, plugin: pluginScope(request.Plugin), cancel: cancel}
	return scoped, func() {
		cancel()
		owner.mu.Lock()
		delete(owner.active, key)
		owner.mu.Unlock()
	}, nil
}

func (owner *Installations) Progress(caller protocol.NodeID, request protocol.RequestID, stage string) {
	owner.mu.Lock()
	defer owner.mu.Unlock()
	if active := owner.active[installKey{caller, request}]; active != nil {
		active.progress = protocol.SshInstallProgress{Kind: stage}
	}
}

func (owner *Installations) Read(caller protocol.NodeID, request protocol.RequestID, plugin *protocol.PluginContext) (protocol.SshInstallProgress, error) {
	owner.mu.Lock()
	defer owner.mu.Unlock()
	active := owner.active[installKey{caller, request}]
	if active == nil || active.progress.Kind == "" || plugin != nil && active.plugin != pluginScope(plugin) {
		return protocol.SshInstallProgress{}, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "Host installation is not active"}
	}
	return active.progress, nil
}

func (owner *Installations) Cancel(caller protocol.NodeID, request protocol.RequestID, plugin *protocol.PluginContext) error {
	owner.mu.Lock()
	defer owner.mu.Unlock()
	active := owner.active[installKey{caller, request}]
	if active == nil {
		return nil
	}
	if plugin != nil && active.plugin != pluginScope(plugin) {
		return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "connection request belongs to another plugin scope"}
	}
	active.cancel()
	return nil
}
