package plugins

import (
	"archive/zip"
	"context"
	"io"
	"os"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// ExportArchive captures verified immutable package content, including packages
// originally installed from a local worktree that another host cannot access.
func (manager *Manager) ExportArchive(ctx context.Context, reference protocol.PluginReference, destination io.Writer) error {
	directory, err := manager.PackageRoot(reference)
	if err != nil {
		return err
	}
	root, err := os.OpenRoot(directory)
	if err != nil {
		return err
	}
	defer root.Close()
	entries, issues, err := capturePackage(ctx, root.FS(), func(name string) (io.ReadCloser, error) { return root.Open(name) })
	if err != nil {
		return err
	}
	info, err := describePackage(entries, issues, reference.Name)
	if err != nil {
		return err
	}
	if info.Summary.Digest != reference.Digest {
		return packageFault("immutable plugin package content changed")
	}
	archive := zip.NewWriter(destination)
	for _, entry := range entries {
		if err := ctx.Err(); err != nil {
			archive.Close()
			return err
		}
		header := &zip.FileHeader{Name: entry.name, Method: zip.Deflate}
		mode := os.FileMode(0600)
		if entry.directory {
			header.Name += "/"
			mode = os.ModeDir | 0700
		} else if entry.executable {
			mode = 0700
		}
		header.SetMode(mode)
		file, err := archive.CreateHeader(header)
		if err != nil {
			archive.Close()
			return err
		}
		if !entry.directory {
			if _, err := file.Write(entry.data); err != nil {
				archive.Close()
				return err
			}
		}
	}
	return archive.Close()
}
