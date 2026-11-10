package preview

import (
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"time"
)

// The settings pages' preview data: what a Node and the app would report
// for each section. Credentials are never part of it; pages show them as
// masked placeholders.

// EnvVar is a terminal environment variable.
type EnvVar struct{ Key, Value string }

// Shell is a terminal shell a host offers; an empty Path is its default.
type Shell struct {
	Path string
	Args []string
}

// Device is a paired host (Execution) or mobile controller.
type Device struct {
	ID, Name, Platform string
	Execution, Online  bool
	Local              bool   // this computer
	Route              string // "p2p", "relay" or "" while offline
	RTT                int    // round trip in ms; 0 for none
}

type HookPhase = viewmodel.HookPhase

const (
	BeforeTool = viewmodel.BeforeTool
	AfterTool  = viewmodel.AfterTool
	TurnEnd    = viewmodel.TurnEnd
)

type Hook = viewmodel.Hook

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

// Settings is the settings pages' dataset.
type Settings struct {
	Version      string
	PreventSleep bool
	Errors       bool // telemetry: error reports
	Metrics      bool // telemetry: usage metrics
	Sandbox      bool
	Env          []EnvVar
	Shells       []Shell
	Devices      []Device
	Relays       []string // the named relay servers
	Hooks        []Hook
	HooksOn      bool
	Python       string // the bundled interpreter's version
	Indexes      []string
	OCR          []LocalModel
	OCRCatalog   []LocalModel // models the catalog offers
	Speech       []LocalModel
	Embedding    []LocalModel // knowledge models
	Decision     []LocalModel
	Microphones  []string
	Permissions  []Permission
	Archived     []Archived
	Sessions     int // sessions that are not archived
}

// SettingsData builds the settings' dataset, fresh each call.
func SettingsData() *Settings {
	return &Settings{
		Version: "0.1.0", Errors: true, Metrics: true, Sandbox: true,
		Env:    []EnvVar{{"EDITOR", "nvim"}, {"LANG", "en_US.UTF-8"}},
		Shells: []Shell{{}, {Path: "/bin/zsh"}, {Path: "/bin/bash"}, {Path: "/opt/homebrew/bin/fish"}},
		Devices: []Device{
			{ID: "7f3a9c21", Name: "MacBook Pro", Platform: "macos", Execution: true, Online: true, Local: true},
			{ID: "c48e0d55", Name: "build-server", Platform: "linux", Execution: true, Online: true, Route: "p2p", RTT: 18},
			{ID: "a1b2c3d4", Name: "Studio PC", Platform: "windows", Execution: true, Route: ""},
			{ID: "5e6f7a8b", Name: "iPhone", Platform: "ios", Online: true, Route: "relay", RTT: 64},
		},
		Relays: []string{"Sailry Asia", "Sailry Europe"},
		Hooks: []Hook{
			{ID: "fmt", Name: "Format", Command: "gofmt -l .", Phase: AfterTool, TimeoutMS: 30000, Enabled: true},
			{ID: "guard", Command: "./scripts/check-paths.sh", Phase: BeforeTool, TimeoutMS: 5000},
			{ID: "notes", Name: "Notes", Command: "git status --short > .notes", Phase: TurnEnd, TimeoutMS: 10000},
		},
		HooksOn: true,
		Python:  "3.12.7",
		Indexes: []string{"pypi.org", "mirrors.aliyun.com", "mirrors.tencent.com", "pypi.tuna.tsinghua.edu.cn", "custom"},
		OCR: []LocalModel{
			{ID: "ppocr", Name: "PP-OCRv5", Repo: "PaddlePaddle/PP-OCRv5_mobile", Status: ModelInstalled},
			{ID: "got", Name: "GOT-OCR 2.0", Repo: "stepfun-ai/GOT-OCR2_0", Status: ModelInstalled},
		},
		OCRCatalog: []LocalModel{
			{ID: "ppocr", Name: "PP-OCRv5", Repo: "PaddlePaddle/PP-OCRv5_mobile", Status: ModelInstalled},
			{ID: "got", Name: "GOT-OCR 2.0", Repo: "stepfun-ai/GOT-OCR2_0", Status: ModelInstalled},
			{ID: "florence", Name: "Florence-2", Repo: "microsoft/Florence-2-base", Status: ModelDownloading, Percent: 42},
			{ID: "trocr", Name: "TrOCR", Repo: "microsoft/trocr-base-printed"},
		},
		Speech: []LocalModel{
			{ID: "", Name: "", Status: ModelInstalled},
			{ID: "whisper-small", Name: "Whisper Small", Repo: "openai/whisper-small", Status: ModelMissing},
			{ID: "sense", Name: "SenseVoice Small", Repo: "FunAudioLLM/SenseVoiceSmall", Status: ModelDownloading, Percent: 63},
		},
		Embedding: []LocalModel{
			{ID: "bge-m3", Name: "BGE-M3", Repo: "BAAI/bge-m3", Status: ModelInstalled},
			{ID: "qwen3-embed", Name: "Qwen3 Embedding 0.6B", Repo: "Qwen/Qwen3-Embedding-0.6B", Status: ModelDownloading, Percent: 27},
			{ID: "e5", Name: "multilingual-e5-small", Repo: "intfloat/multilingual-e5-small"},
		},
		Decision: []LocalModel{
			{ID: "qwen3-0.6b", Name: "Qwen3 0.6B", Repo: "Qwen/Qwen3-0.6B", Status: ModelInstalled},
			{ID: "gemma-3-1b", Name: "Gemma 3 1B", Repo: "google/gemma-3-1b-it"},
		},
		Microphones: []string{"MacBook Pro Microphone", "AirPods Pro"},
		Permissions: []Permission{
			{"permission_accessibility", "permission_granted", true},
			{"permission_screen", "permission_required", true},
			{"permission_microphone", "permission_granted", true},
			{"permission_full_disk", "permission_unknown", true},
			{"permission_keychain", "permission_not_needed", false},
		},
		Archived: []Archived{
			{"arch-1", "Refactor the updater", Now.Add(-26 * time.Hour)},
			{"arch-2", "Plan the relay probe", Now.Add(-9 * 24 * time.Hour)},
		},
		Sessions: 12,
	}
}
