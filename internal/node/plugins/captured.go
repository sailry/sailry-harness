package plugins

import (
	"context"
	"io"
	"os"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (manager *Manager) captured(ctx context.Context, reference protocol.PluginReference) ([]packageEntry, error) {
	directory, err := manager.PackageRoot(reference)
	if err != nil {
		return nil, err
	}
	root, err := core.OpenFiles(directory, core.ManualFiles)
	if err != nil {
		return nil, err
	}
	defer root.Close()
	tree, err := os.OpenRoot(directory)
	if err != nil {
		return nil, err
	}
	defer tree.Close()
	entries, issues, err := capturePackage(ctx, tree.FS(), func(name string) (io.ReadCloser, error) { return root.Open(name) })
	if err != nil {
		return nil, err
	}
	if len(issues) != 0 || digestPackage(entries) != reference.Digest {
		return nil, packageFault("immutable plugin package content changed")
	}
	return entries, nil
}

func (manager *Manager) CapturedInfo(ctx context.Context, reference protocol.PluginReference) (protocol.PluginInfo, error) {
	entries, err := manager.captured(ctx, reference)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	info, err := describePackage(entries, nil, reference.Name)
	info.Summary.SettingsRevision = reference.SettingsRevision
	return info, err
}
