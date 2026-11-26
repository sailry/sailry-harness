package node

import (
	"context"
	"io/fs"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) readArtifactView(ctx context.Context, reference protocol.PluginReference) (*protocol.ArtifactViewContent, error) {
	if view, ok := node.modules.ArtifactViewFor(reference); ok {
		return plugins.ResolveArtifactView(ctx, view.View, func(name string) ([]byte, error) {
			return fs.ReadFile(view.Tree, name)
		})
	}
	return node.plugins.manager.ReadArtifactView(ctx, reference)
}

func (node *Node) artifactClasses(ctx context.Context, reference protocol.PluginReference) []string {
	if view, ok := node.modules.ArtifactViewFor(reference); ok {
		return view.View.AllowedClasses
	}
	info, err := node.plugins.manager.CapturedInfo(node.pluginContext(ctx), reference)
	if err != nil || info.Extension == nil || info.Extension.ArtifactView == nil {
		return nil
	}
	return info.Extension.ArtifactView.AllowedClasses
}
