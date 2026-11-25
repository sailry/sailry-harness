package testplugin

import (
	"archive/zip"
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"slices"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type Package struct {
	Name     string
	Platform string
	MCP      string
	Files    map[string]string
}

func Basic(name string) Package {
	return Package{Name: name, Platform: `{"api_version":"v1","display":{"label":"Local fixture","icon":"phosphor:puzzle-piece"}}`}
}

func (fixture Package) Contents() (map[string]string, error) {
	manifest := map[string]any{
		"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
		"name":    fixture.Name, "version": "1.0.0", "description": "Deterministic Harness test fixture",
		"extensions": map[string]json.RawMessage{"dev.sailry.platform": json.RawMessage(fixture.Platform)},
	}
	body, err := json.Marshal(manifest)
	if err != nil {
		return nil, err
	}
	files := map[string]string{"plugin.json": string(body)}
	if fixture.MCP != "" {
		files["mcp.json"] = fixture.MCP
	}
	for name, value := range fixture.Files {
		if !fs.ValidPath(name) || name == "plugin.json" {
			return nil, fmt.Errorf("invalid fixture resource %q", name)
		}
		files[name] = value
	}
	return files, nil
}

func (fixture Package) Write(root string) (string, error) {
	files, err := fixture.Contents()
	if err != nil {
		return "", err
	}
	directory := filepath.Join(root, fixture.Name)
	for name, value := range files {
		filename := filepath.Join(directory, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(filename), 0o700); err != nil {
			return "", err
		}
		if err := os.WriteFile(filename, []byte(value), 0o600); err != nil {
			return "", err
		}
	}
	return directory, nil
}

// Install exercises the same staged ZIP transfer and durable installation as a
// local Desktop package, without depending on a worktree or shipped inventory.
func Install(ctx context.Context, controller *client.Client, fixture Package) (protocol.PluginInfo, error) {
	files, err := fixture.Contents()
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	var archive bytes.Buffer
	writer := zip.NewWriter(&archive)
	names := make([]string, 0, len(files))
	for name := range files {
		names = append(names, name)
	}
	slices.Sort(names)
	for _, name := range names {
		entry, err := writer.Create(name)
		if err != nil {
			return protocol.PluginInfo{}, err
		}
		if _, err := io.WriteString(entry, files[name]); err != nil {
			return protocol.PluginInfo{}, err
		}
	}
	if err := writer.Close(); err != nil {
		return protocol.PluginInfo{}, err
	}
	data := archive.Bytes()
	size, revision := uint64(len(data)), fmt.Sprintf("%x", blake3.Sum256(data))
	output, err := controller.Do(ctx, protocol.Command{Kind: "upload_plugin", Data: protocol.PluginUploadSpec{Size: size, Revision: revision}})
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	var upload protocol.PluginUpload
	if err := decode(output, "plugin_upload", &upload); err != nil {
		return protocol.PluginInfo{}, err
	}
	source := io.NopCloser(bytes.NewReader(data))
	defer source.Close()
	if err := controller.Upload(ctx, upload.Stream, size, revision, source, nil); err != nil {
		return protocol.PluginInfo{}, err
	}
	output, err = controller.Do(ctx, protocol.Command{Kind: "install_plugin_upload", Data: protocol.CommandInstallPluginUploadPayload{
		Stream: upload.Stream, Source: protocol.PluginUploadSourceArchive, Name: fixture.Name,
	}})
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	var info protocol.PluginInfo
	if err := decode(output, "plugin", &info); err != nil {
		return info, err
	}
	if info.Summary.Name != fixture.Name || !info.Summary.Enabled || info.Summary.Compatibility.Kind != "compatible" || len(info.Issues) != 0 {
		return info, fmt.Errorf("fixture %q was not installed ready: %+v, issues %+v", fixture.Name, info.Summary, info.Issues)
	}
	return info, nil
}

func decode(output protocol.Output, kind string, target any) error {
	if output.Kind != kind {
		return fmt.Errorf("expected %s, received %s", kind, output.Kind)
	}
	body, err := json.Marshal(output.Data)
	if err != nil {
		return err
	}
	return json.Unmarshal(body, target)
}
