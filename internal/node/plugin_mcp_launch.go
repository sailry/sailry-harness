package node

import (
	"context"
	"encoding/json"
	"errors"
	"io/fs"
	"maps"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/computer"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/python"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type mcpLaunchOwner struct {
	packageRoot *os.Root
	dataRoot    *os.Root
	projection  string
	python      *python.Ready
	once        sync.Once
	err         error
}

func (owner *mcpLaunchOwner) close() error {
	owner.once.Do(func() {
		var failures []error
		if owner.projection != "" && owner.dataRoot != nil {
			if err := owner.dataRoot.Remove(owner.projection); err != nil && !errors.Is(err, fs.ErrNotExist) {
				failures = append(failures, err)
			}
		}
		if owner.python != nil {
			failures = append(failures, owner.python.Close())
		}
		if owner.dataRoot != nil {
			failures = append(failures, owner.dataRoot.Close())
		}
		if owner.packageRoot != nil {
			failures = append(failures, owner.packageRoot.Close())
		}
		if errors.Join(failures...) != nil {
			owner.err = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "MCP launch resources could not be released"}
		}
	})
	return owner.err
}

func (node *Node) prepareMCPLaunch(ctx context.Context, info protocol.PluginInfo, settings store.ResolvedPluginSettings, name string, server protocol.PluginMcpServer) (prepared mcp.Configuration, resultErr error) {
	prepared.Package, prepared.Server, prepared.Options = plugins.PackageReference(info), name, mcp.ServerOptions(server)
	prepared.Launch.Kind = server.Kind
	prepared.Authorization = map[string]any{"settings": settings.Secrets}
	if err := plugins.ValidateMCPServer(server); err != nil {
		return prepared, err
	}
	switch value := server.Data.(type) {
	case *protocol.PluginMcpServerHttpPayload:
		prepared.Launch.URL, prepared.Launch.Headers, prepared.Tools = value.Url, copyMCPValues(value.Headers), value.Tools
	case *protocol.PluginMcpServerSsePayload:
		prepared.Launch.URL, prepared.Launch.Headers, prepared.Tools = value.Url, copyMCPValues(value.Headers), value.Tools
	case *protocol.PluginMcpServerStdioPayload:
		owner := &mcpLaunchOwner{}
		prepared.Release = owner.close
		defer func() {
			if resultErr != nil {
				resultErr = errors.Join(resultErr, owner.close())
			}
		}()
		packagePath, err := node.plugins.manager.PackageRoot(prepared.Package)
		if err != nil {
			return prepared, err
		}
		owner.packageRoot, err = os.OpenRoot(packagePath)
		if err != nil {
			return prepared, err
		}
		profile, err := core.OpenFiles(node.profile.Ownership.Path, core.ManualFiles)
		if err != nil {
			return prepared, err
		}
		err = profile.CreatePrivateDirectories("plugins/data/" + info.Summary.Name)
		profile.Close()
		if err != nil {
			return prepared, err
		}
		dataPath := filepath.Join(node.profile.Ownership.Path, "plugins", "data", info.Summary.Name)
		owner.dataRoot, err = os.OpenRoot(dataPath)
		if err != nil {
			return prepared, err
		}
		basePath, relative, err := mcpDirectory(value.Cwd, packagePath, dataPath)
		if err != nil {
			return prepared, err
		}
		base := owner.packageRoot
		if basePath == dataPath {
			base = owner.dataRoot
		}
		directory, err := base.OpenRoot(filepath.FromSlash(relative))
		if err != nil {
			return prepared, err
		}
		directory.Close()
		launch := &prepared.Launch
		launch.Directory = filepath.Join(basePath, filepath.FromSlash(relative))
		launch.Command = value.Command
		if value.Command == computer.Token {
			if launch.Command, err = computer.Executable(); err != nil {
				return prepared, err
			}
		}
		if relativeCommand, found := strings.CutPrefix(value.Command, "./"); found {
			file, err := owner.packageRoot.Open(filepath.FromSlash(relativeCommand))
			if err != nil {
				return prepared, err
			}
			metadata, statErr := file.Stat()
			file.Close()
			if statErr != nil || !metadata.Mode().IsRegular() {
				return prepared, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "MCP command is not a regular package file"}
			}
			launch.Command = filepath.Join(packagePath, filepath.FromSlash(relativeCommand))
		}
		launch.Args = slices.Clone(value.Args)
		for index := range launch.Args {
			launch.Args[index] = expandMCP(launch.Args[index], packagePath, dataPath)
		}
		launch.Env = copyMCPValues(value.Env)
		for key, content := range launch.Env {
			launch.Env[key] = expandMCP(content, packagePath, dataPath)
		}
		launch.Env["PLUGIN_ROOT"], launch.Env["PLUGIN_DATA"] = packagePath, dataPath
		if value.Command == computer.Token {
			computer.Session(launch.Env)
			if home, declared := launch.Env["AGENT_DESKTOP_HOME"]; declared {
				if err := computer.PrepareHome(dataPath, home); err != nil {
					return prepared, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "computer helper home is unavailable"}
				}
			}
		}
		environment := node.environment.TerminalValues()
		launch.Environment = slices.Clone(environment)
		slices.Sort(launch.Environment)
		if info.Settings != nil {
			owner.projection = ".sailry-settings-" + protocol.NewRequestID().String() + ".json"
			file, err := owner.dataRoot.OpenFile(owner.projection, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
			if err != nil {
				return prepared, err
			}
			body, marshalErr := json.Marshal(settings.Values)
			if marshalErr == nil {
				_, marshalErr = file.Write(body)
			}
			if err := errors.Join(marshalErr, file.Close()); err != nil {
				return prepared, err
			}
			launch.Env["SAILRY_PLUGIN_SETTINGS_FILE"] = filepath.Join(dataPath, owner.projection)
		}
		prepared.Tools = value.Tools
		if value.Command == "python" || value.Command == "python3" {
			var requirements *python.Requirements
			content, err := node.plugins.manager.ReadResource(ctx, prepared.Package, "requirements.txt", 512*1024)
			if err == nil {
				requirements, err = python.NewRequirements(prepared.Package, string(content))
				if err != nil {
					return prepared, err
				}
			} else {
				var fault *protocol.Fault
				if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeNotFound {
					return prepared, err
				}
			}
			owner.python, err = node.plugins.python.Ensure(ctx, requirements)
			if err != nil {
				return prepared, err
			}
			path := ""
			for _, assignment := range environment {
				key, content, found := strings.Cut(assignment, "=")
				if found && strings.EqualFold(key, "PATH") {
					path = content
				}
			}
			launch.Command, launch.Args, launch.Env, err = owner.python.MCP(value.Command, launch.Args, launch.Env, path)
			if err != nil {
				return prepared, err
			}
		}
	}
	if info.Settings != nil {
		targets := store.MCPSettingsTargets(info.Settings, protocol.PluginMcpConfiguration{Servers: map[string]protocol.PluginMcpServer{name: server}})
		for field, secret := range settings.Secrets {
			binding := info.Settings.Properties[field].Secret
			if binding == nil || binding.Server == nil || *binding.Server != name {
				continue
			}
			if settings.MCPTargets[field] != targets[field] {
				return prepared, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "MCP secret destination changed; update its settings"}
			}
			content := secret.Expose()
			if binding.Prefix != nil {
				content = *binding.Prefix + content
			}
			if binding.Env != nil {
				if _, exists := prepared.Launch.Env[*binding.Env]; !exists {
					return prepared, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "MCP secret environment slot is unavailable"}
				}
				prepared.Launch.Env[*binding.Env] = content
			}
			if binding.Header != nil {
				found := false
				for header := range prepared.Launch.Headers {
					if strings.EqualFold(header, *binding.Header) {
						prepared.Launch.Headers[header] = content
						found = true
					}
				}
				if !found {
					return prepared, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "MCP secret header slot is unavailable"}
				}
			}
		}
	}
	base := prepared.Launch
	base.Env, base.Headers = maps.Clone(base.Env), maps.Clone(base.Headers)
	authorization, err := node.authorizeMCP(ctx, info, name, &prepared.Launch)
	if err != nil {
		return prepared, err
	}
	prepared.Authorization = []any{prepared.Authorization, authorization}
	capture := connectorMCPAuthority{info: info, settings: settings, server: name, target: server, base: base, launch: prepared.Launch, identity: authorization}
	prepared.Validate = func(ctx context.Context) error { return node.validateMCPAuthorization(ctx, capture) }
	return prepared, nil
}

func copyMCPValues(values map[string]string) map[string]string {
	copy := make(map[string]string, len(values))
	for name, value := range values {
		copy[name] = value
	}
	return copy
}
