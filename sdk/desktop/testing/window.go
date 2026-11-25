package testing

import (
	"context"
	"errors"
	"fmt"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/live"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	native "github.com/sailry/sailry-harness/sdk/native/node"
)

// Window owns a headless Desktop binding and Client, but not its supplied Node.
type Window struct {
	*live.Runtime
	Controller *client.Client
}

// Connect binds a product-selected Desktop to a Node owned by the test.
func Connect(ctx context.Context, owner *native.Node, profile string, assembly shell.Assembly) (*Window, error) {
	controller := client.New(link.NewLocal(owner.ID(), owner.ID(), owner))
	if err := controller.Start(ctx); err != nil {
		_ = controller.Close()
		return nil, err
	}
	runtime, err := live.NewWithAssembly(ctx, controller, preferences.ForStartup(profile), assembly)
	if err != nil {
		_ = controller.Close()
		return nil, err
	}
	return &Window{Runtime: runtime, Controller: controller}, nil
}

func (window *Window) Close() error {
	return errors.Join(window.Runtime.Close(), window.Controller.Close())
}

func (window *Window) SetPluginEnabled(ctx context.Context, name string, enabled bool) error {
	for _, summary := range window.Controller.Snapshot().Plugins {
		if summary.Name == name {
			_, err := window.Controller.Do(ctx, protocol.Command{Kind: "set_plugin_enabled", Data: protocol.CommandSetPluginEnabledPayload{
				Name: name, ExpectedRevision: summary.Revision, Enabled: enabled,
			}})
			return err
		}
	}
	return fmt.Errorf("test Node has no %s package", name)
}
