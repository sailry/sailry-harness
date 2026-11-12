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
// decisions. Version (the source's revision), License and Size are its
// definition's, "" where unknown. Removable marks a model whose files can
// be deleted and, once installed, updated.
type LocalModel struct {
	ID, Name, Repo         string
	Status                 ModelStatus
	Percent                float64 // of a download
	Version, License, Size string
	Removable              bool
}

// KnowledgeState is what a knowledge collection is doing.
type KnowledgeState int

const (
	KnowledgePending  KnowledgeState = iota // waiting for its indexing pass
	KnowledgeIndexing                       // being read
	KnowledgeReady                          // indexed
	KnowledgePaused                         // paused by the user
	KnowledgeFailed                         // could not be indexed as a whole
)

// KnowledgeCollection is a host's knowledge collection: its state, the
// sources indexed so far, those that could not be indexed (Failed) with
// the first one's Reason, or the collection's own when it failed as a
// whole, and whether an action on it is under way.
type KnowledgeCollection struct {
	ID, Name string
	State    KnowledgeState
	Sources  uint64
	Failed   uint64
	Reason   string
	Busy     bool
}

// LibraryModel is a model the model library finds (model_library rows).
type LibraryModel struct {
	ID, Name, Repo, Kind  string // Kind: model_library_embedding, _ocr, _speech, model_library_decision
	Source, Size, License string
	Status                ModelStatus
	Percent               float64
	// Versions are the hub's versions once read, newest first, and Version
	// the index of the one the details show. Resolved marks a known
	// definition (size, license), which an install from the details needs;
	// Current marks the shown version as the one the host has, which Status
	// is about. QueryPrefix and DocumentPrefix are an embedding model's own.
	Versions                    []string
	Version                     int
	Resolved, Current           bool
	QueryPrefix, DocumentPrefix string
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
