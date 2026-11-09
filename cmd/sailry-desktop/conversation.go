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

func bindConversation(ctx context.Context, entry *desktopWindow, controller *client.Client, speech *reader.Service) {
	runtime := entry.live
	runtime.SetReaderActive(speech.Active)
	runtime.SetReadAloud(func(session, turn, text string) {
		if err := speech.Toggle(turn, text); err != nil {
			runtime.ReportReaderFailure()
		}
	})
	runtime.SetExport(func(sessionText, turnText string, selected conversation.ExportFormat) {
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
			assets := strings.TrimSuffix(filepath.Base(destination), filepath.Ext(destination)) + "-assets"
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
			if err == nil && len(document.Attachments) > 0 {
				err = &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "attachment export requires the attachment transfer service"}
			}
			var data []byte
			if err == nil {
				data, err = exporter.Render(ctx, entry.window, document, assets, nil, format)
			}
			if err == nil {
				err = saveConversation(destination, data)
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
