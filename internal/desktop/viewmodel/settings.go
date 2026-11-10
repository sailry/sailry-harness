package viewmodel

import "time"

// Shell is a terminal shell a host offers; an empty Path is its default.
type Shell struct {
	Path string
	Args []string
}

// ModelStatus is a local model's state on its host.
type ModelStatus int

const (
	ModelMissing ModelStatus = iota
	ModelDownloading
	ModelInstalled
)

// LocalModel is a model a host runs for OCR, dictation, knowledge or
// decisions.
type LocalModel struct {
	ID, Name, Repo string
	Status         ModelStatus
	Percent        float64 // of a download
}

// LibraryModel is a model the model library finds (model_library rows).
type LibraryModel struct {
	ID, Name, Repo, Kind  string // Kind: model_library_embedding, _ocr, _speech, model_library_decision
	Source, Size, License string
	Status                ModelStatus
	Percent               float64
}

// Permission is a system permission the app or a plugin needs, by its
// i18n keys (permission_screen; permission_granted).
type Permission struct {
	Resource, Status string
	Settings         bool // the system has a settings page for it
}

// Archived is an archived session.
type Archived struct {
	ID, Title string
	At        time.Time
}
