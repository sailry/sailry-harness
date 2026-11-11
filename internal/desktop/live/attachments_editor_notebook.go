package live

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/MoeForge/moego/base64"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (p *workspaceProject) readEditorNotebook(path string, offset uint64) {
	previous := p.panel.EditorNotebook(path)
	if previous != nil && previous.Loading {
		return
	}
	draft, dirty := p.panel.FileDraft(path)
	page := &viewmodel.EditorNotebook{Path: path, Loading: true}
	if offset > 0 && previous != nil {
		*page = *previous
		page.Loading, page.Failed = true, false
	}
	p.panel.SetEditorNotebook(path, page)
	owner, previousRevision := p.owner.ID, page.Revision
	var revision string
	if file := p.files[path]; file.Revision != nil {
		revision = *file.Revision
	}
	started := p.runtime.async(func() {
		var loaded *viewmodel.EditorNotebook
		var err error
		if dirty {
			loaded, err = projectNotebookDraft(path, revision, draft)
		} else {
			var output protocol.Output
			output, err = p.runtime.controller.Do(p.runtime.ctx, protocol.Command{Kind: "read_notebook", Data: protocol.CommandReadNotebookPayload{Worktree: owner, Options: protocol.NotebookRead{Path: path, Offset: offset, Limit: 20}}})
			if err == nil {
				value, ok := detail[protocol.NotebookPage](output.Data)
				if !ok || output.Kind != "notebook_page" || value.Path != path {
					err = fileActionFault(protocol.ErrorCodeInvalidRequest, "notebook response does not match the requested file")
				} else if revision != "" && revision != value.Revision || offset > 0 && previousRevision != value.Revision {
					err = fileActionFault(protocol.ErrorCodeRevisionConflict, "notebook revision changed")
				} else {
					loaded, err = projectNotebookPage(value)
				}
			}
		}
		p.runtime.enqueue(func(c *ui.Context) {
			if p.panel.EditorNotebook(path) != page {
				return
			}
			current, currentDirty := p.panel.FileDraft(path)
			if current != draft || currentDirty != dirty {
				p.panel.ResetEditorNotebook(path)
				return
			}
			if err == nil && !dirty {
				if file := p.files[path]; file.Revision != nil && *file.Revision != loaded.Revision {
					err = fileActionFault(protocol.ErrorCodeRevisionConflict, "notebook revision changed")
				}
			}
			if err != nil {
				p.panel.EditorNotebookFailed(path, page, dirty)
				p.runtime.reportKeys(c, err, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
				return
			}
			if offset > 0 && !dirty {
				loaded.Cells = append(append([]viewmodel.EditorNotebookCell(nil), page.Cells...), loaded.Cells...)
			}
			p.panel.SetEditorNotebook(path, loaded)
		})
	})
	if !started {
		p.panel.EditorNotebookFailed(path, page, dirty)
	}
}

func projectNotebookDraft(path, revision, source string) (*viewmodel.EditorNotebook, error) {
	var notebook struct {
		Format   uint32            `json:"nbformat"`
		Metadata json.RawMessage   `json:"metadata"`
		Cells    []json.RawMessage `json:"cells"`
	}
	if json.Unmarshal([]byte(source), &notebook) != nil {
		return nil, fileActionFault(protocol.ErrorCodeInvalidRequest, "notebook draft is not valid JSON")
	}
	metadata := strings.TrimSpace(string(notebook.Metadata))
	if notebook.Format != 4 || len(metadata) == 0 || metadata[0] != '{' || notebook.Cells == nil {
		return nil, fileActionFault(protocol.ErrorCodeInvalidRequest, "notebook draft must use nbformat 4 with metadata and cells")
	}
	page := protocol.NotebookPage{Path: path, Revision: revision, Metadata: notebook.Metadata}
	for index, raw := range notebook.Cells {
		var cell struct {
			Kind   string          `json:"cell_type"`
			Source json.RawMessage `json:"source"`
		}
		if json.Unmarshal(raw, &cell) != nil || cell.Kind != "code" && cell.Kind != "markdown" && cell.Kind != "raw" || !notebookSourceValid(cell.Source) {
			return nil, fileActionFault(protocol.ErrorCodeInvalidRequest, "invalid notebook draft cell")
		}
		page.Cells = append(page.Cells, protocol.NotebookCell{Index: uint64(index), Value: raw})
	}
	return projectNotebookPage(page)
}

func notebookSourceValid(raw json.RawMessage) bool {
	var text string
	if json.Unmarshal(raw, &text) == nil && strings.TrimSpace(string(raw)) != "null" {
		return true
	}
	var lines []json.RawMessage
	if json.Unmarshal(raw, &lines) != nil || lines == nil {
		return false
	}
	for _, line := range lines {
		if strings.TrimSpace(string(line)) == "null" || json.Unmarshal(line, &text) != nil {
			return false
		}
	}
	return true
}
func notebookText(raw json.RawMessage) string {
	var text string
	if json.Unmarshal(raw, &text) == nil {
		return text
	}
	var lines []string
	if json.Unmarshal(raw, &lines) == nil {
		return strings.Join(lines, "")
	}
	return ""
}
func projectNotebookPage(page protocol.NotebookPage) (*viewmodel.EditorNotebook, error) {
	view := &viewmodel.EditorNotebook{Path: page.Path, Revision: page.Revision, Next: page.NextOffset}
	var metadata struct {
		Language struct {
			Name string `json:"name"`
		} `json:"language_info"`
		Kernel struct {
			Language string `json:"language"`
		} `json:"kernelspec"`
	}
	_ = json.Unmarshal(page.Metadata, &metadata)
	view.Language = metadata.Language.Name
	if view.Language == "" {
		view.Language = metadata.Kernel.Language
	}
	for _, raw := range page.Cells {
		var cell struct {
			Kind    string          `json:"cell_type"`
			Source  json.RawMessage `json:"source"`
			Outputs json.RawMessage `json:"outputs"`
		}
		if json.Unmarshal(raw.Value, &cell) != nil {
			return nil, fileActionFault(protocol.ErrorCodeInvalidRequest, "invalid notebook cell response")
		}
		projected := viewmodel.EditorNotebookCell{Index: raw.Index, Kind: cell.Kind, Source: notebookText(cell.Source), Truncated: raw.Truncated}
		var outputs []json.RawMessage
		_ = json.Unmarshal(cell.Outputs, &outputs)
		for outputIndex, rawOutput := range outputs {
			var output map[string]json.RawMessage
			if json.Unmarshal(rawOutput, &output) != nil {
				continue
			}
			var data map[string]json.RawMessage
			_ = json.Unmarshal(output["data"], &data)
			var text string
			switch notebookText(output["output_type"]) {
			case "stream":
				text = notebookText(output["text"])
			case "error":
				var traceback []json.RawMessage
				if json.Unmarshal(output["traceback"], &traceback) == nil && traceback != nil {
					lines := make([]string, len(traceback))
					for index, line := range traceback {
						lines[index] = notebookText(line)
					}
					text = strings.Join(lines, "\n")
				} else {
					text = notebookText(output["ename"]) + ": " + notebookText(output["evalue"])
				}
			default:
				text = notebookText(data["text/plain"])
			}
			if text != "" {
				projected.Outputs = append(projected.Outputs, text)
			}
			for _, media := range []string{"image/png", "image/jpeg"} {
				encoded := notebookText(data[media])
				if encoded == "" || len(encoded) > 1024*1024 {
					continue
				}
				encoded = strings.Map(func(r rune) rune {
					if r == ' ' || r == '\t' || r == '\n' || r == '\r' || r == '\v' || r == '\f' {
						return -1
					}
					return r
				}, encoded)
				data, err := base64.StdEncoding.DecodeString(encoded)
				if err != nil {
					continue
				}
				bitmap, err := decodeAttachmentBitmap(data, 1024)
				if err != nil {
					continue
				}
				width, height := bitmap.Size()
				projected.Images = append(projected.Images, conversation.Image{ID: fmt.Sprintf("notebook/%d/%d", raw.Index, outputIndex), Width: width, Height: height, Source: bitmap})
				break
			}
		}
		view.Cells = append(view.Cells, projected)
	}
	return view, nil
}
