package viewmodel

import "github.com/sailry/sailry-harness/internal/desktop/conversation"

type EditorNotebook struct {
	Path, Revision, Language string
	Cells                    []EditorNotebookCell
	Next                     *uint64
	Loading                  bool
	Failed                   bool
}
type EditorNotebookCell struct {
	Index        uint64
	Kind, Source string
	Outputs      []string
	Images       []conversation.Image
	Truncated    bool
}
