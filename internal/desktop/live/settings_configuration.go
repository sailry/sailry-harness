package live

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Settings › Data: the shown Node's configuration copied through a file.
// The actions share the backup page's Node, binding and busy state.

// configurationFileLimit is the largest configuration file the page reads.
const configurationFileLimit = 8 << 20

// configurationActions are the page's export and import of the Node it shows.
func (r *Runtime) configurationActions() *settings.ConfigurationActions {
	state := &r.backupSettings
	ready := func() *Runtime {
		owner := state.shown
		if !state.visible || owner == nil || r.managedSettingsNode() != owner || !owner.controller.Connected() ||
			owner.machineSettingsStatus() != "" || owner.backupSettings.busy || owner.backupSettings.archive != "" || !owner.backupSettings.known {
			return nil
		}
		return owner
	}
	failed := func(key string) {
		r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, key)) })
	}
	return &settings.ConfigurationActions{
		Export: func() {
			owner := ready()
			if owner == nil {
				return
			}
			if r.attachments == nil || r.attachments.native.SaveFile == nil {
				failed("files_download_picker")
				return
			}
			r.attachments.native.SaveFile("sailry-configuration.json", func(path string) {
				if path != "" {
					r.enqueue(func(*ui.Context) {
						if ready() == owner {
							owner.exportConfiguration(path)
						}
					})
				}
			})
		},
		Pick: func(selected func([]byte)) {
			owner := ready()
			if owner == nil {
				return
			}
			if r.attachments == nil || r.attachments.native.PickFiles == nil {
				failed("files_upload_picker")
				return
			}
			binding := state.binding
			r.attachments.native.PickFiles(func(paths []string) {
				if len(paths) == 0 {
					return
				}
				document, problem := readConfigurationFile(paths[0])
				r.enqueue(func(c *ui.Context) {
					if state.binding != binding || ready() != owner {
						return
					}
					if problem != "" {
						kit.Toast(c, kit.Failed, i18n.T(c, problem))
						return
					}
					selected(document)
				})
			})
		},
		Import: func(document []byte, preview bool, done func([]settings.ConfigurationItem, bool, bool)) {
			owner := ready()
			if owner == nil {
				done(nil, false, false)
				return
			}
			binding := state.binding
			owner.importConfiguration(document, preview, func(items []settings.ConfigurationItem, err error) {
				if state.binding == binding && state.shown == owner {
					done(items, err == nil, client.Retained(err))
				}
			})
		},
		Forget: func() {
			if owner := state.shown; owner != nil {
				owner.forgetConfigurationImport()
			}
		},
	}
}

// readConfigurationFile is a picked file's document, or the key of why it
// is not one.
func readConfigurationFile(path string) ([]byte, string) {
	file, err := os.Open(path)
	if err != nil {
		return nil, "configuration_read_failed"
	}
	defer file.Close()
	document, err := io.ReadAll(io.LimitReader(file, configurationFileLimit+1))
	switch {
	case err != nil:
		return nil, "configuration_read_failed"
	case len(document) > configurationFileLimit:
		return nil, "configuration_too_large"
	case !json.Valid(document):
		return nil, "configuration_invalid"
	}
	return document, ""
}

// exportConfiguration writes the Node's configuration to path. The file
// appears there only once it is complete, readable by its owner alone.
func (r *Runtime) exportConfiguration(path string) {
	s := &r.backupSettings
	s.busy = r.async(func() {
		err := r.writeConfiguration(r.ctx, path)
		r.enqueue(func(c *ui.Context) {
			s.work, s.busy = "", false
			// The export reads; only writing the file can fail its way.
			outcome, key := kit.Done, "configuration_exported"
			switch {
			case err == nil:
			case faultCode(err) == protocol.ErrorCodePermissionDenied:
				outcome, key = kit.Failed, "backup_denied"
			default:
				outcome, key = kit.Failed, "configuration_export_failed"
			}
			kit.Toast(c, outcome, i18n.T(c, key))
		})
	})
	if s.busy {
		s.work = "configuration-export"
	}
}

func (r *Runtime) writeConfiguration(ctx context.Context, path string) error {
	document, err := r.exportedConfiguration(ctx)
	if err != nil {
		return err
	}
	return writeConfigurationFile(ctx, path, document)
}

// exportedConfiguration is the Node's configuration document as the
// export command returns it, apart from where it is kept.
func (r *Runtime) exportedConfiguration(ctx context.Context) ([]byte, error) {
	output, err := r.controller.Do(ctx, protocol.Command{Kind: "export_configuration"})
	if err != nil {
		return nil, err
	}
	value, ok := detail[protocol.ConfigurationDocument](output.Data)
	if output.Kind != "configuration_document" || !ok {
		return nil, fmt.Errorf("unexpected configuration document response")
	}
	return value.Document, nil
}

// writeConfigurationFile writes document to path, indented. The file
// appears there only once it is complete, readable by its owner alone.
func writeConfigurationFile(ctx context.Context, path string, document []byte) error {
	var pretty bytes.Buffer
	if err := json.Indent(&pretty, document, "", "  "); err != nil {
		return err
	}
	pretty.WriteByte('\n')
	target, err := prepareDownloadDestination(path)
	if err != nil {
		return err
	}
	defer target.close()
	if _, err = target.file.Write(pretty.Bytes()); err != nil {
		return err
	}
	return target.publish(ctx)
}

// configurationImportCommand is the import of document; a preview changes
// nothing on the Node.
func configurationImportCommand(document []byte, preview bool) protocol.Command {
	return protocol.Command{Kind: "import_configuration", Data: protocol.CommandImportConfigurationPayload{Document: json.RawMessage(document), Preview: preview}}
}

// importConfiguration sends document and reports its items. An import
// whose outcome is unknown is kept, so its dialog's Retry sends the same
// request; its outcome is a toast, and a preview's success shows none.
func (r *Runtime) importConfiguration(document []byte, preview bool, finished func([]settings.ConfigurationItem, error)) {
	s := &r.backupSettings
	command := configurationImportCommand(document, preview)
	s.busy, s.work = true, "configuration-import"
	r.execute(command, func(c *ui.Context, output protocol.Output, err error) {
		s.work, s.busy = "", false
		report, ok := detail[protocol.ConfigurationImportReport](output.Data)
		if err == nil && (output.Kind != "configuration_import" || !ok) {
			err = fmt.Errorf("unexpected configuration import response")
		}
		if !preview {
			s.imported = nil
			if client.Retained(err) {
				s.imported = &command
			}
		}
		switch {
		case err != nil:
			outcome, key := configurationFailure(err)
			kit.Toast(c, outcome, i18n.T(c, key))
		case !preview:
			// The import changed settings other pages and this one show.
			s.stale = true
			r.readSettings()
			kit.Toast(c, kit.Done, i18n.T(c, "configuration_imported"))
		}
		finished(configurationItems(report), err)
	})
}

// forgetConfigurationImport drops an import kept for Retry, so the same
// document imported later is a new request.
func (r *Runtime) forgetConfigurationImport() {
	if s := &r.backupSettings; s.imported != nil {
		r.forgetCommand(*s.imported)
		s.imported = nil
	}
}

// configurationItems are a report's items as the page lists them.
func configurationItems(report protocol.ConfigurationImportReport) []settings.ConfigurationItem {
	items := make([]settings.ConfigurationItem, 0, len(report.Items))
	for _, item := range report.Items {
		row := settings.ConfigurationItem{Area: item.Area, Name: item.Name, Action: string(item.Action)}
		if item.Reason != nil {
			row.Reason = *item.Reason
		}
		items = append(items, row)
	}
	return items
}

// configurationFailure is a failed export's or import's toast.
func configurationFailure(err error) (kit.Outcome, string) {
	switch faultCode(err) {
	case protocol.ErrorCodeRevisionConflict, protocol.ErrorCodeConflict:
		return kit.Conflict, "configuration_conflict"
	case protocol.ErrorCodeInvalidRequest:
		return kit.Failed, "configuration_invalid"
	case protocol.ErrorCodePermissionDenied:
		return kit.Failed, "backup_denied"
	case protocol.ErrorCodeOutcomeUnknown:
		return kit.Uncertain, "configuration_unknown"
	}
	return kit.Failed, "configuration_failed"
}
