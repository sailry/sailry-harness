package plugins

import (
	"context"
	"errors"
	"io/fs"
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
	files := make(map[string][]byte, len(entries))
	for _, entry := range entries {
		if !entry.directory {
			files[entry.name] = entry.data
		}
	}
	return ResolveArtifactView(ctx, *info.Extension.ArtifactView, func(name string) ([]byte, error) {
		if content, exists := files[name]; exists {
			return content, nil
		}
		return nil, fs.ErrNotExist
	})
}

// ResolveArtifactView reads a declaration from a verified package capture or
// the embedded files of a native module, enforcing the same resource budget.
func ResolveArtifactView(ctx context.Context, declared protocol.ArtifactView, readFile func(string) ([]byte, error)) (*protocol.ArtifactViewContent, error) {
	remaining := protocol.PluginDesktopMaxBytes
	read := func(name string) (string, error) {
		content, err := readFile(name)
		if errors.Is(err, fs.ErrNotExist) {
			return "", &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "artifact view resource is unavailable"}
		}
		if err != nil {
			return "", err
		}
		if len(content) > remaining || !utf8.Valid(content) {
			return "", packageFault("artifact view resources are invalid or exceed limit")
		}
		remaining -= len(content)
		return string(content), nil
	}
	view := protocol.DefaultArtifactViewContent()
	var err error
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
