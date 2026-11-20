package plugins

import (
	"context"
	"slices"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// ReadArtifactView returns the renderer a captured package declares for its
// session documents: the template, styles and scripts in declared order, and
// the names of the visual libraries the controller supplies. A package
// without a renderer returns nil.
func (manager *Manager) ReadArtifactView(ctx context.Context, reference protocol.PluginReference) (*protocol.ArtifactViewContent, error) {
	entries, err := manager.captured(ctx, reference)
	if err != nil {
		return nil, err
	}
	info, err := describePackage(entries, nil, reference.Name)
	if err != nil {
		return nil, err
	}
	if info.Extension == nil || info.Extension.ArtifactView == nil {
		return nil, nil
	}
	declared := info.Extension.ArtifactView
	files := make(map[string][]byte, len(entries))
	for _, entry := range entries {
		if !entry.directory {
			files[entry.name] = entry.data
		}
	}
	remaining := protocol.PluginDesktopMaxBytes
	read := func(name string) (string, error) {
		content, exists := files[name]
		if !exists {
			return "", &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "plugin artifact view resource is unavailable"}
		}
		if len(content) > remaining || !utf8.Valid(content) {
			return "", packageFault("plugin artifact view resources are invalid or exceed limit")
		}
		remaining -= len(content)
		return string(content), nil
	}
	view := protocol.DefaultArtifactViewContent()
	if view.Template, err = read(declared.Template); err != nil {
		return nil, err
	}
	for _, name := range declared.Styles {
		style, err := read(name)
		if err != nil {
			return nil, err
		}
		view.Styles = append(view.Styles, style)
	}
	for _, name := range declared.Scripts {
		script, err := read(name)
		if err != nil {
			return nil, err
		}
		view.Scripts = append(view.Scripts, script)
	}
	view.Libraries = slices.Clone(declared.Libraries)
	return &view, ctx.Err()
}
