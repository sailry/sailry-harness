package node

import (
	"context"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func (node *Node) readConfigurationPackage(ctx context.Context, caller protocol.NodeID, input protocol.ConfigurationFollowCommand) (protocol.Output, error) {
	if input.Package == nil || input.Source == nil || *input.Source != caller || caller == node.ID() {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "package source does not match authenticated caller"}
	}
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		user, err := store.CurrentUser(ctx, db)
		if err != nil {
			return err
		}
		member, err := store.CallerUser(ctx, db, node.ID(), caller)
		if err != nil || user.ID != member.ID {
			return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "configuration source is not a member of this user"}
		}
		status, err := store.ReadConfigurationFollow(ctx, db)
		if err != nil {
			return err
		}
		if status.Source != nil && *status.Source != caller || status.Source == nil && status.Revision != 0 {
			return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "configuration belongs to another source"}
		}
		return nil
	})
	if err != nil {
		return protocol.Output{}, err
	}
	available := false
	if input.Action == "package_state" {
		if _, err = node.plugins.manager.PackageRoot(*input.Package); err == nil {
			err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				var exists bool
				err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM configuration_follow_packages WHERE digest=?1 AND name=?2)", input.Package.Digest, input.Package.Name).Scan(&exists)
				available = exists
				return err
			})
		}
		return protocol.Output{Kind: "configuration_package", Data: protocol.ConfigurationPackageStatus{Available: available}}, nil
	}
	if input.Stream == nil {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "package upload stream is required"}
	}
	var info protocol.PluginInfo
	err = node.transfers.UsePluginUpload(ctx, caller, *input.Stream, true, func(reader io.ReaderAt, size int64) error {
		var err error
		info, err = node.plugins.manager.InstallArchive(ctx, reader, size, input.Package.Name)
		return err
	})
	if err != nil {
		return protocol.Output{}, err
	}
	if info.Summary.Name != input.Package.Name || info.Summary.Digest != input.Package.Digest {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "followed package digest does not match"}
	}
	declared := protocol.PluginMcpConfiguration{Schema: "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json", Servers: map[string]protocol.PluginMcpServer{}}
	if len(info.Mcp) > 0 {
		declared, err = node.declaredPluginMCP(ctx, plugins.PackageReference(info))
		if err != nil {
			return protocol.Output{}, err
		}
	}
	err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error { return store.CacheConfigurationPackage(ctx, db, info, declared) })
	return protocol.Output{Kind: "configuration_package", Data: protocol.ConfigurationPackageStatus{Available: err == nil}}, err
}

func (node *Node) prepareConfigurationPackages(ctx context.Context, remote link.Transport, manifest protocol.ConfigurationManifest) error {
	if manifest.Category != "plugins" {
		return nil
	}
	for _, record := range manifest.Records {
		if len(record.Body) == 0 {
			continue
		}
		var info protocol.PluginInfo
		if err := json.Unmarshal(record.Body, &info); err != nil {
			return err
		}
		reference := plugins.PackageReference(info)
		command := protocol.ConfigurationFollowCommand{Action: "package_state", Source: pointerNode(node.ID()), Package: &reference}
		output, err := configurationRemote(ctx, remote, protocol.NewRequest(remote.Target(), protocol.Command{Kind: "configuration_follow", Data: &command}))
		if err != nil {
			return err
		}
		var status protocol.ConfigurationPackageStatus
		if err := decodeConfigurationOutput(output, &status); err != nil {
			return err
		}
		if status.Available {
			continue
		}
		directory := filepath.Join(node.profile.Ownership.Path, "tmp")
		if err := os.MkdirAll(directory, 0700); err != nil {
			return err
		}
		file, err := os.CreateTemp(directory, "configuration-package-")
		if err != nil {
			return err
		}
		err = func() error {
			defer os.Remove(file.Name())
			defer file.Close()
			if err := node.plugins.manager.ExportArchive(ctx, reference, file); err != nil {
				return err
			}
			if _, err := file.Seek(0, io.SeekStart); err != nil {
				return err
			}
			digest := blake3.New(32, nil)
			size, err := io.Copy(digest, file)
			if err != nil {
				return err
			}
			if _, err := file.Seek(0, io.SeekStart); err != nil {
				return err
			}
			revision := hex.EncodeToString(digest.Sum(nil))
			output, err := configurationRemote(ctx, remote, protocol.NewRequest(remote.Target(), protocol.Command{Kind: "upload_plugin", Data: &protocol.PluginUploadSpec{Size: uint64(size), Revision: revision}}))
			if err != nil {
				return err
			}
			var upload protocol.PluginUpload
			if err := decodeConfigurationOutput(output, &upload); err != nil {
				return err
			}
			controller := client.New(remote)
			defer controller.Close()
			if err := controller.Upload(ctx, upload.Stream, uint64(size), revision, file, nil); err != nil {
				return err
			}
			command.Action, command.Stream = "cache_package", &upload.Stream
			_, err = configurationRemote(ctx, remote, protocol.NewRequest(remote.Target(), protocol.Command{Kind: "configuration_follow", Data: &command}))
			return err
		}()
		if err != nil {
			return err
		}
	}
	return nil
}
func decodeConfigurationOutput(output protocol.Output, value any) error {
	body, err := json.Marshal(output.Data)
	if err != nil {
		return err
	}
	if err := json.Unmarshal(body, value); err != nil {
		return errors.New("configuration host returned invalid output")
	}
	return nil
}
