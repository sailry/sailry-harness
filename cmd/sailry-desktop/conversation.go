package main

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"unicode"

	"github.com/egoist/mygo"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/exporter"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/reader"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func bindConversation(ctx context.Context, entry *desktopWindow, speech *reader.Service) {
	runtime := entry.live
	runtime.SetReaderActive(speech.Active)
	runtime.SetReadAloud(func(session, turn, text string) {
		if err := speech.Toggle(turn, text, runtime.ReportReaderFailure); err != nil {
			runtime.ReportReaderFailure()
		}
	})
	runtime.SetExport(func(sessionText, turnText string, selected conversation.ExportFormat) {
		controller, ok := runtime.SessionClient(sessionText)
		if !ok {
			runtime.ReportExport(fmt.Errorf("conversation execution owner is unavailable"))
			return
		}
		session, err := protocol.ParseSessionID(sessionText)
		if err != nil {
			runtime.ReportExport(err)
			return
		}
		title := "Sailry"
		for _, candidate := range controller.Snapshot().Sessions {
			if candidate.ID == session {
				title = candidate.Activity.Title
				break
			}
		}
		format := exporter.Markdown
		key := "conversation_export_markdown"
		switch selected {
		case conversation.ExportHTML:
			format = exporter.HTML
			key = "conversation_export_html"
		case conversation.ExportImage:
			format = exporter.PNG
			key = "conversation_export_image"
		}
		name := strings.Map(func(r rune) rune {
			if unicode.IsControl(r) || strings.ContainsRune(`/\:*?"<>|`, r) {
				return '-'
			}
			return r
		}, title)
		name = strings.TrimSpace(name)
		if name == "" || name == "." || name == ".." {
			name = "Sailry"
		}
		name = string([]rune(name)[:min(len([]rune(name)), 120)])
		lang := i18n.Match(mygo.App.Locale())
		destination, err := mygo.Dialog.Save(mygo.SaveDialogOptions{Parent: entry.window, Title: i18n.In(lang, "conversation_export"), DefaultPath: name + "." + format.Extension(), Filters: []mygo.FileFilter{{Name: i18n.In(lang, key), Extensions: []string{format.Extension()}}}, CreateDirectories: true})
		if err != nil {
			runtime.ReportExport(err)
			return
		}
		if destination == "" {
			return
		}
		go func() {
			assets := "sailry-assets-" + protocol.NewRequestID().String()
			documentFormat := client.ExportMarkdown
			if format != exporter.Markdown {
				documentFormat = client.ExportHTML
			}
			var document client.ExportDocument
			var err error
			if turnText == "" {
				document, err = controller.ExportConversation(ctx, session, title, documentFormat, assets)
			} else {
				var turn protocol.TurnID
				turn, err = protocol.ParseTurnID(turnText)
				if err == nil {
					document, err = controller.ExportReply(ctx, session, turn, title, documentFormat, assets)
				}
			}
			files := map[string][]byte{}
			for _, attachment := range document.Attachments {
				if err != nil {
					break
				}
				var data []byte
				image := false
				for _, candidate := range document.Images {
					if candidate.Attachment.ID == attachment.ID {
						data, err = controller.ImageBytes(ctx, session, candidate)
						image = true
						break
					}
				}
				if !image {
					data, err = controller.AttachmentBytes(ctx, attachment)
				}
				if err == nil {
					files[client.ExportAttachmentName(attachment)] = data
				}
			}

			var data []byte
			if err == nil {
				data, err = exporter.Render(ctx, entry.window, document, assets, files, format)
			}
			if err == nil {
				err = saveConversationAssets(ctx, destination, data, assets, files, format)
			}
			runtime.ReportExport(err)
		}()
	})
}

// Publication uses a sibling temporary file, preserving a previous destination
// if writing or syncing fails. The system save panel owns overwrite consent.
func saveConversation(destination string, data []byte) error {
	temporary, err := os.CreateTemp(filepath.Dir(destination), ".sailry-export-")
	if err != nil {
		return err
	}
	name := temporary.Name()
	defer os.Remove(name)
	if err = temporary.Chmod(0600); err == nil {
		_, err = temporary.Write(data)
	}
	if err == nil {
		err = temporary.Sync()
	}
	closeErr := temporary.Close()
	if err == nil {
		err = closeErr
	}
	if err != nil {
		return err
	}
	if err = os.Rename(name, destination); err != nil {
		return fmt.Errorf("publish conversation export: %w", err)
	}
	return nil
}

// Markdown publishes immutable assets before its document; native renders are standalone.
func saveConversationAssets(ctx context.Context, destination string, data []byte, assets string, files map[string][]byte, format exporter.Format) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if format != exporter.Markdown || len(files) == 0 {
		return saveConversation(destination, data)
	}
	staging, err := os.MkdirTemp(filepath.Dir(destination), ".sailry-export-assets-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(staging)
	for name, body := range files {
		if filepath.Base(name) != name {
			return fmt.Errorf("invalid export asset name")
		}
		file, err := os.OpenFile(filepath.Join(staging, name), os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
		if err != nil {
			return err
		}
		_, err = file.Write(body)
		if err == nil {
			err = file.Sync()
		}
		closed := file.Close()
		if err != nil {
			return err
		}
		if closed != nil {
			return closed
		}
	}
	if err = ctx.Err(); err != nil {
		return err
	}
	published := filepath.Join(filepath.Dir(destination), assets)
	if _, err = os.Lstat(published); !os.IsNotExist(err) {
		return fmt.Errorf("export asset destination already exists")
	}
	if err = os.Rename(staging, published); err != nil {
		return err
	}
	if err = saveConversation(destination, data); err != nil {
		_ = os.RemoveAll(published)
	}
	return err
}
