package settings

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// The kinds of local model, named by the model library's kind keys
// (preview.LibraryModel.Kind).
const (
	ModelsEmbedding = "model_library_embedding"
	ModelsOCR       = "model_library_ocr"
	ModelsSpeech    = "model_library_speech"
	ModelsDecision  = "model_library_decision"
)

// ModelActions are the live actions of the local-model pages (Dictation's
// speech models, OCR, Knowledge, Decision) and the model library. The
// preview has none: its pages change in memory and its downloads are
// preview_notice. kind is one of the Models* keys; a managed row is named by
// its model's ID and a library row by its LibraryModel.ID.
type ModelActions struct {
	// Save sends the page's switch and chosen model as the page shows them.
	Save func(kind string)
	// Select chooses an installed model.
	Select func(kind, id string)
	// Install downloads a registered model; Cancel stops its download.
	Install func(kind, id string)
	Cancel  func(kind, id string)
	// Update looks for a newer version of an installed model and downloads
	// it; Delete removes a registered model after the confirmation.
	Update func(kind, id string)
	Delete func(kind, id string)
	// Microphone asks the system for microphone access.
	Microphone func()
	// The library's rows: Install resolves, registers and downloads the
	// model, Cancel stops its download, Delete removes its files after the
	// confirmation, Details reads its versions, size and license. An
	// install from the details passes its choice: the version shown and,
	// for an embedding model, the prefixes typed; a row's install passes
	// nil. Version shows another of the model's versions in the details;
	// More reads the hub's next page.
	LibraryInstall func(id string, choice *LibraryChoice)
	LibraryCancel  func(id string)
	LibraryDelete  func(id string)
	LibraryDetails func(id string)
	LibraryVersion func(id string, version int)
	LibraryMore    func()
	// The Knowledge page's collections: Pause sets or clears a collection's
	// pause, Update reads its sources again.
	CollectionPause  func(id string, paused bool)
	CollectionUpdate func(id string)
}

// LibraryChoice is what the library's details install: the version they
// show, with an embedding model's prefixes as typed.
type LibraryChoice struct{ QueryPrefix, DocumentPrefix string }

// CollectionMessages are the toasts of a collection's pause and update;
// the row shows that either went through.
var CollectionMessages = Messages{Failed: "runtime_failed", Conflict: "runtime_conflict", Uncertain: "runtime_unknown"}

// LocalModelMessages are the toasts of a local-model settings save.
var LocalModelMessages = Messages{Done: "knowledge_saved", Failed: "runtime_failed", Conflict: "runtime_conflict", Uncertain: "runtime_unknown"}

// LibraryRequest is what the model library dialog asks for: the kind it
// was opened for, the hub (0 Hugging Face, 1 ModelScope) and the query as
// searched. Serial counts the searches the user ran, so that the same
// query searches again.
type LibraryRequest struct {
	Open   bool
	Kind   string
	Hub    int
	Query  string
	Serial uint64
}

// LibraryRequest reports the library dialog's request.
func (m *Model) LibraryRequest() LibraryRequest {
	e := &m.library
	return LibraryRequest{Open: e.open, Kind: e.kind, Hub: e.hub, Query: strings.TrimSpace(e.searched), Serial: e.serial}
}

// CloseModelDialogs closes the library and the models' details, as when
// the page turns to another host.
func (m *Model) CloseModelDialogs() {
	m.library, m.info, m.dropping = library{}, nil, nil
}

// openLibrary opens the library for a page's kind. A live library starts
// empty on the kind's hub (ModelScope for OCR) with its first page on the
// way; the preview's keeps its query.
func (m *Model) openLibrary(kind string) {
	if m.Live {
		m.library = library{}
		if kind == ModelsOCR {
			m.library.hub = 1
		}
		m.Library, m.LibraryLoad = nil, preview.Fetch{Loading: true}
		m.LibraryMore, m.LibraryFailed = false, false
	}
	m.library.open, m.library.kind = true, kind
}

// modelAction runs a live action; in the preview it is preview_notice.
func (m *Model) modelAction(c *ui.Context, run func(*ModelActions)) {
	if !m.Live {
		notice(c)
	} else if m.ModelActions != nil {
		run(m.ModelActions)
	}
}

// followModel keeps the details dialog on the model's current state while
// a live download changes it.
func (m *Model) followModel(models []preview.LocalModel) {
	if !m.Live || m.info == nil {
		return
	}
	for _, x := range models {
		if x.ID == m.info.model.ID {
			m.info.model = x
		}
	}
}
