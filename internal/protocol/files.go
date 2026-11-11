// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type FileUploadSpec struct {
	Worktree         WorktreeID `json:"worktree"`
	Path             string     `json:"path"`
	Size             uint64     `json:"size"`
	Revision         string     `json:"revision"`
	ExpectedRevision *string    `json:"expected_revision"`
}

func DefaultFileUploadSpec() FileUploadSpec {
	return FileUploadSpec{
		Worktree:         WorktreeID{},
		Path:             "",
		Size:             0,
		Revision:         "",
		ExpectedRevision: nil,
	}
}
func (value *FileUploadSpec) UnmarshalJSON(data []byte) error {
	type plain FileUploadSpec
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "path", "size", "revision"}, false); err != nil {
		return err
	}
	*value = FileUploadSpec(decoded)
	return nil
}
func (value FileUploadSpec) MarshalJSON() ([]byte, error) {
	type plain FileUploadSpec
	encoded := plain(value)
	return json.Marshal(encoded)
}

type FileUpload struct {
	Stream StreamID       `json:"stream"`
	Spec   FileUploadSpec `json:"spec"`
}

func DefaultFileUpload() FileUpload {
	return FileUpload{
		Stream: StreamID{},
		Spec:   DefaultFileUploadSpec(),
	}
}
func (value *FileUpload) UnmarshalJSON(data []byte) error {
	type plain FileUpload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"stream", "spec"}, false); err != nil {
		return err
	}
	*value = FileUpload(decoded)
	return nil
}
func (value FileUpload) MarshalJSON() ([]byte, error) {
	type plain FileUpload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type FileDownload struct {
	Stream   StreamID   `json:"stream"`
	Worktree WorktreeID `json:"worktree"`
	Path     string     `json:"path"`
	Size     uint64     `json:"size"`
	Revision string     `json:"revision"`
	Stamp    string     `json:"stamp"`
}

func DefaultFileDownload() FileDownload {
	return FileDownload{
		Stream:   StreamID{},
		Worktree: WorktreeID{},
		Path:     "",
		Size:     0,
		Revision: "",
		Stamp:    "",
	}
}
func (value *FileDownload) UnmarshalJSON(data []byte) error {
	type plain FileDownload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"stream", "worktree", "path", "size", "revision", "stamp"}, false); err != nil {
		return err
	}
	*value = FileDownload(decoded)
	return nil
}
func (value FileDownload) MarshalJSON() ([]byte, error) {
	type plain FileDownload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type FileSearch struct {
	Query         string     `json:"query"`
	Regex         bool       `json:"regex"`
	CaseSensitive bool       `json:"case_sensitive"`
	Globs         []string   `json:"globs"`
	Path          string     `json:"path"`
	Mode          SearchMode `json:"mode"`
	Context       uint64     `json:"context"`
	FileType      *string    `json:"type"`
	Limit         uint64     `json:"limit"`
	Offset        uint64     `json:"offset"`
	Multiline     bool       `json:"multiline"`
}

func DefaultFileSearch() FileSearch {
	return FileSearch{
		Query:         "",
		Regex:         false,
		CaseSensitive: false,
		Globs:         []string{},
		Path:          "",
		Mode:          SearchModeContent,
		Context:       0,
		FileType:      nil,
		Limit:         200,
		Offset:        0,
		Multiline:     false,
	}
}
func (value *FileSearch) UnmarshalJSON(data []byte) error {
	type plain FileSearch
	decoded := plain(DefaultFileSearch())
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = FileSearch(decoded)
	return nil
}
func (value FileSearch) MarshalJSON() ([]byte, error) {
	type plain FileSearch
	encoded := plain(value)
	encoded.Globs = normalizeCollections(encoded.Globs)
	return json.Marshal(encoded)
}

type SearchMode string

const (
	SearchModeContent SearchMode = "content"
	SearchModeFiles   SearchMode = "files"
	SearchModeCount   SearchMode = "count"
)

func (value SearchMode) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *SearchMode) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch SearchMode(decoded) {
	case SearchModeContent, SearchModeFiles, SearchModeCount:
		*value = SearchMode(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value SearchMode) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *SearchMode) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type SearchMatch struct {
	Path       string `json:"path"`
	LineNumber uint64 `json:"line_number"`
	Line       string `json:"line"`
	Matched    bool   `json:"matched"`
	Truncated  bool   `json:"truncated"`
}

func DefaultSearchMatch() SearchMatch {
	return SearchMatch{
		Path:       "",
		LineNumber: 0,
		Line:       "",
		Matched:    false,
		Truncated:  false,
	}
}
func (value *SearchMatch) UnmarshalJSON(data []byte) error {
	type plain SearchMatch
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "line_number", "line", "matched", "truncated"}, false); err != nil {
		return err
	}
	*value = SearchMatch(decoded)
	return nil
}
func (value SearchMatch) MarshalJSON() ([]byte, error) {
	type plain SearchMatch
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SearchCount struct {
	Path  string `json:"path"`
	Count uint64 `json:"count"`
}

func DefaultSearchCount() SearchCount {
	return SearchCount{
		Path:  "",
		Count: 0,
	}
}
func (value *SearchCount) UnmarshalJSON(data []byte) error {
	type plain SearchCount
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "count"}, false); err != nil {
		return err
	}
	*value = SearchCount(decoded)
	return nil
}
func (value SearchCount) MarshalJSON() ([]byte, error) {
	type plain SearchCount
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SearchDiagnostic struct {
	Path    string `json:"path"`
	Message string `json:"message"`
}

func DefaultSearchDiagnostic() SearchDiagnostic {
	return SearchDiagnostic{
		Path:    "",
		Message: "",
	}
}
func (value *SearchDiagnostic) UnmarshalJSON(data []byte) error {
	type plain SearchDiagnostic
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "message"}, false); err != nil {
		return err
	}
	*value = SearchDiagnostic(decoded)
	return nil
}
func (value SearchDiagnostic) MarshalJSON() ([]byte, error) {
	type plain SearchDiagnostic
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SearchResults struct {
	Mode         SearchMode         `json:"mode"`
	Matches      []SearchMatch      `json:"matches"`
	Paths        []string           `json:"paths"`
	Counts       []SearchCount      `json:"counts"`
	ScannedFiles uint64             `json:"scanned_files"`
	Skipped      uint64             `json:"skipped"`
	Truncated    bool               `json:"truncated"`
	Partial      bool               `json:"partial"`
	Offset       uint64             `json:"offset"`
	NextOffset   *uint64            `json:"next_offset"`
	Diagnostics  []SearchDiagnostic `json:"diagnostics"`
}

func DefaultSearchResults() SearchResults {
	return SearchResults{
		Mode:         SearchModeContent,
		Matches:      []SearchMatch{},
		Paths:        []string{},
		Counts:       []SearchCount{},
		ScannedFiles: 0,
		Skipped:      0,
		Truncated:    false,
		Partial:      false,
		Offset:       0,
		NextOffset:   nil,
		Diagnostics:  []SearchDiagnostic{},
	}
}
func (value *SearchResults) UnmarshalJSON(data []byte) error {
	type plain SearchResults
	decoded := plain(DefaultSearchResults())
	if err := DecodeObject(data, &decoded, []string{}, false); err != nil {
		return err
	}
	*value = SearchResults(decoded)
	return nil
}
func (value SearchResults) MarshalJSON() ([]byte, error) {
	type plain SearchResults
	encoded := plain(value)
	encoded.Matches = normalizeCollections(encoded.Matches)
	encoded.Paths = normalizeCollections(encoded.Paths)
	encoded.Counts = normalizeCollections(encoded.Counts)
	encoded.Diagnostics = normalizeCollections(encoded.Diagnostics)
	fields := map[string]any{"mode": encoded.Mode, "scanned_files": encoded.ScannedFiles, "skipped": encoded.Skipped, "truncated": encoded.Truncated, "partial": encoded.Partial, "offset": encoded.Offset, "next_offset": encoded.NextOffset, "diagnostics": encoded.Diagnostics}
	switch encoded.Mode {
	case SearchModeContent:
		fields["matches"] = encoded.Matches
	case SearchModeFiles:
		fields["paths"] = encoded.Paths
	case SearchModeCount:
		fields["counts"] = encoded.Counts
	default:
		return nil, fmt.Errorf("unknown search mode %q", encoded.Mode)
	}
	return json.Marshal(fields)
}

type EntryKind string

const (
	EntryKindFile      EntryKind = "file"
	EntryKindDirectory EntryKind = "directory"
	EntryKindSymlink   EntryKind = "symlink"
	EntryKindOther     EntryKind = "other"
)

func (value EntryKind) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *EntryKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch EntryKind(decoded) {
	case EntryKindFile, EntryKindDirectory, EntryKindSymlink, EntryKindOther:
		*value = EntryKind(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value EntryKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *EntryKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type FileEntry struct {
	ModifiedMs *uint64   `json:"modified_ms,omitempty"`
	Name       string    `json:"name"`
	Kind       EntryKind `json:"kind"`
	Size       uint64    `json:"size"`
}

func DefaultFileEntry() FileEntry {
	return FileEntry{
		Name: "",
		Kind: EntryKind(""),
		Size: 0,
	}
}
func (value *FileEntry) UnmarshalJSON(data []byte) error {
	type plain FileEntry
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "kind", "size"}, false); err != nil {
		return err
	}
	*value = FileEntry(decoded)
	return nil
}
func (value FileEntry) MarshalJSON() ([]byte, error) {
	type plain FileEntry
	encoded := plain(value)
	return json.Marshal(encoded)
}

type DirectoryCursor struct {
	Revision  string `json:"revision"`
	Directory bool   `json:"directory"`
	Name      string `json:"name"`
}

func DefaultDirectoryCursor() DirectoryCursor {
	return DirectoryCursor{
		Revision:  "",
		Directory: false,
		Name:      "",
	}
}
func (value *DirectoryCursor) UnmarshalJSON(data []byte) error {
	type plain DirectoryCursor
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"revision", "directory", "name"}, false); err != nil {
		return err
	}
	*value = DirectoryCursor(decoded)
	return nil
}
func (value DirectoryCursor) MarshalJSON() ([]byte, error) {
	type plain DirectoryCursor
	encoded := plain(value)
	return json.Marshal(encoded)
}

type Directory struct {
	Path             string           `json:"path"`
	Entries          []FileEntry      `json:"entries"`
	Truncated        bool             `json:"truncated"`
	UnsupportedNames uint64           `json:"unsupported_names"`
	Revision         string           `json:"revision"`
	Next             *DirectoryCursor `json:"next"`
}

func DefaultDirectory() Directory {
	return Directory{
		Path:             "",
		Entries:          []FileEntry{},
		Truncated:        false,
		UnsupportedNames: 0,
		Revision:         "",
		Next:             nil,
	}
}
func (value *Directory) UnmarshalJSON(data []byte) error {
	type plain Directory
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "entries", "truncated", "unsupported_names", "revision"}, false); err != nil {
		return err
	}
	*value = Directory(decoded)
	return nil
}
func (value Directory) MarshalJSON() ([]byte, error) {
	type plain Directory
	encoded := plain(value)
	encoded.Entries = normalizeCollections(encoded.Entries)
	return json.Marshal(encoded)
}

type FileContent struct {
	Path      string  `json:"path"`
	Text      string  `json:"text"`
	Size      uint64  `json:"size"`
	Truncated bool    `json:"truncated"`
	Revision  *string `json:"revision"`
}

func DefaultFileContent() FileContent {
	return FileContent{
		Path:      "",
		Text:      "",
		Size:      0,
		Truncated: false,
		Revision:  nil,
	}
}
func (value *FileContent) UnmarshalJSON(data []byte) error {
	type plain FileContent
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "text", "size", "truncated"}, false); err != nil {
		return err
	}
	*value = FileContent(decoded)
	return nil
}
func (value FileContent) MarshalJSON() ([]byte, error) {
	type plain FileContent
	encoded := plain(value)
	return json.Marshal(encoded)
}

type FileLine struct {
	Number uint64 `json:"number"`
	Text   string `json:"text"`
}

func DefaultFileLine() FileLine {
	return FileLine{
		Number: 0,
		Text:   "",
	}
}
func (value *FileLine) UnmarshalJSON(data []byte) error {
	type plain FileLine
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"number", "text"}, false); err != nil {
		return err
	}
	*value = FileLine(decoded)
	return nil
}
func (value FileLine) MarshalJSON() ([]byte, error) {
	type plain FileLine
	encoded := plain(value)
	return json.Marshal(encoded)
}

type FileLines struct {
	Path       string     `json:"path"`
	Lines      []FileLine `json:"lines"`
	Size       uint64     `json:"size"`
	Revision   string     `json:"revision"`
	Offset     uint64     `json:"offset"`
	NextOffset *uint64    `json:"next_offset"`
	Truncated  bool       `json:"truncated"`
}

func DefaultFileLines() FileLines {
	return FileLines{
		Path:       "",
		Lines:      []FileLine{},
		Size:       0,
		Revision:   "",
		Offset:     0,
		NextOffset: nil,
		Truncated:  false,
	}
}
func (value *FileLines) UnmarshalJSON(data []byte) error {
	type plain FileLines
	decoded := plain{}
	decoded.Truncated = false
	if err := DecodeObject(data, &decoded, []string{"path", "lines", "size", "revision", "offset"}, false); err != nil {
		return err
	}
	*value = FileLines(decoded)
	return nil
}
func (value FileLines) MarshalJSON() ([]byte, error) {
	type plain FileLines
	encoded := plain(value)
	encoded.Lines = normalizeCollections(encoded.Lines)
	return json.Marshal(encoded)
}

type ImageFacts struct {
	MimeType string `json:"mime_type"`
	Width    uint32 `json:"width"`
	Height   uint32 `json:"height"`
	Size     uint64 `json:"size"`
}

func DefaultImageFacts() ImageFacts {
	return ImageFacts{
		MimeType: "",
		Width:    0,
		Height:   0,
		Size:     0,
	}
}
func (value *ImageFacts) UnmarshalJSON(data []byte) error {
	type plain ImageFacts
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"mime_type", "width", "height", "size"}, false); err != nil {
		return err
	}
	*value = ImageFacts(decoded)
	return nil
}
func (value ImageFacts) MarshalJSON() ([]byte, error) {
	type plain ImageFacts
	encoded := plain(value)
	return json.Marshal(encoded)
}

type FileImage struct {
	Path       string        `json:"path"`
	Revision   string        `json:"revision"`
	Source     ImageFacts    `json:"source"`
	Sent       *ImageFacts   `json:"sent,omitempty"`
	InlineData []InlineImage `json:"inline_data"`
}

func DefaultFileImage() FileImage {
	return FileImage{
		Path:       "",
		Revision:   "",
		Source:     DefaultImageFacts(),
		Sent:       nil,
		InlineData: []InlineImage{},
	}
}
func (value *FileImage) UnmarshalJSON(data []byte) error {
	type plain FileImage
	decoded := plain{}
	decoded.Sent = nil
	if err := DecodeObject(data, &decoded, []string{"path", "revision", "source", "inline_data"}, false); err != nil {
		return err
	}
	*value = FileImage(decoded)
	return nil
}
func (value FileImage) MarshalJSON() ([]byte, error) {
	type plain FileImage
	encoded := plain(value)
	encoded.InlineData = normalizeCollections(encoded.InlineData)
	return json.Marshal(encoded)
}

type InlineImage struct {
	MimeType string `json:"mime_type"`
	Data     string `json:"data"`
}

func DefaultInlineImage() InlineImage {
	return InlineImage{
		MimeType: "",
		Data:     "",
	}
}
func (value *InlineImage) UnmarshalJSON(data []byte) error {
	type plain InlineImage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"mime_type", "data"}, false); err != nil {
		return err
	}
	*value = InlineImage(decoded)
	return nil
}
func (value InlineImage) MarshalJSON() ([]byte, error) {
	type plain InlineImage
	encoded := plain(value)
	return json.Marshal(encoded)
}

type FileNames struct {
	Paths       []string           `json:"paths"`
	Visited     uint64             `json:"visited"`
	Skipped     uint64             `json:"skipped"`
	Truncated   bool               `json:"truncated"`
	Partial     bool               `json:"partial"`
	Offset      uint64             `json:"offset"`
	NextOffset  *uint64            `json:"next_offset"`
	Diagnostics []SearchDiagnostic `json:"diagnostics"`
}

func DefaultFileNames() FileNames {
	return FileNames{
		Paths:       []string{},
		Visited:     0,
		Skipped:     0,
		Truncated:   false,
		Partial:     false,
		Offset:      0,
		NextOffset:  nil,
		Diagnostics: []SearchDiagnostic{},
	}
}
func (value *FileNames) UnmarshalJSON(data []byte) error {
	type plain FileNames
	decoded := plain(DefaultFileNames())
	if err := DecodeObject(data, &decoded, []string{}, false); err != nil {
		return err
	}
	*value = FileNames(decoded)
	return nil
}
func (value FileNames) MarshalJSON() ([]byte, error) {
	type plain FileNames
	encoded := plain(value)
	encoded.Paths = normalizeCollections(encoded.Paths)
	encoded.Diagnostics = normalizeCollections(encoded.Diagnostics)
	return json.Marshal(encoded)
}

type FileWritten struct {
	Path     string       `json:"path"`
	Revision string       `json:"revision"`
	Size     uint64       `json:"size"`
	Excerpt  *FileExcerpt `json:"excerpt,omitempty"`
}

func DefaultFileWritten() FileWritten {
	return FileWritten{
		Path:     "",
		Revision: "",
		Size:     0,
		Excerpt:  nil,
	}
}
func (value *FileWritten) UnmarshalJSON(data []byte) error {
	type plain FileWritten
	decoded := plain{}
	decoded.Excerpt = nil
	if err := DecodeObject(data, &decoded, []string{"path", "revision", "size"}, false); err != nil {
		return err
	}
	*value = FileWritten(decoded)
	return nil
}
func (value FileWritten) MarshalJSON() ([]byte, error) {
	type plain FileWritten
	encoded := plain(value)
	return json.Marshal(encoded)
}

type FileExcerpt struct {
	Hunks   []FileHunk `json:"hunks"`
	Omitted uint64     `json:"omitted"`
}

func DefaultFileExcerpt() FileExcerpt {
	return FileExcerpt{
		Hunks:   []FileHunk{},
		Omitted: 0,
	}
}
func (value *FileExcerpt) UnmarshalJSON(data []byte) error {
	type plain FileExcerpt
	decoded := plain{}
	decoded.Omitted = 0
	if err := DecodeObject(data, &decoded, []string{"hunks"}, false); err != nil {
		return err
	}
	*value = FileExcerpt(decoded)
	return nil
}
func (value FileExcerpt) MarshalJSON() ([]byte, error) {
	type plain FileExcerpt
	encoded := plain(value)
	encoded.Hunks = normalizeCollections(encoded.Hunks)
	return json.Marshal(encoded)
}

type FileHunk struct {
	Lines     []FileLine `json:"lines"`
	Shortened bool       `json:"shortened"`
}

func DefaultFileHunk() FileHunk {
	return FileHunk{
		Lines:     []FileLine{},
		Shortened: false,
	}
}
func (value *FileHunk) UnmarshalJSON(data []byte) error {
	type plain FileHunk
	decoded := plain{}
	decoded.Shortened = false
	if err := DecodeObject(data, &decoded, []string{"lines"}, false); err != nil {
		return err
	}
	*value = FileHunk(decoded)
	return nil
}
func (value FileHunk) MarshalJSON() ([]byte, error) {
	type plain FileHunk
	encoded := plain(value)
	encoded.Lines = normalizeCollections(encoded.Lines)
	return json.Marshal(encoded)
}

type FileEdit struct {
	OldText    string `json:"old_text"`
	NewText    string `json:"new_text"`
	ReplaceAll bool   `json:"replace_all,omitempty"`
}

func DefaultFileEdit() FileEdit {
	return FileEdit{
		OldText:    "",
		NewText:    "",
		ReplaceAll: false,
	}
}
func (value *FileEdit) UnmarshalJSON(data []byte) error {
	type plain FileEdit
	decoded := plain{}
	decoded.ReplaceAll = false
	if err := DecodeObject(data, &decoded, []string{"old_text", "new_text"}, true); err != nil {
		return err
	}
	*value = FileEdit(decoded)
	return nil
}
func (value FileEdit) MarshalJSON() ([]byte, error) {
	type plain FileEdit
	encoded := plain(value)
	return json.Marshal(encoded)
}

type FileChange struct {
	Kind string
	Data any
}
type FileChangeCreatePayload struct {
	Path string `json:"path"`
	Text string `json:"text"`
}

func DefaultFileChangeCreatePayload() FileChangeCreatePayload {
	return FileChangeCreatePayload{
		Path: "",
		Text: "",
	}
}
func (value *FileChangeCreatePayload) UnmarshalJSON(data []byte) error {
	type plain FileChangeCreatePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "text"}, true); err != nil {
		return err
	}
	*value = FileChangeCreatePayload(decoded)
	return nil
}
func (value FileChangeCreatePayload) MarshalJSON() ([]byte, error) {
	type plain FileChangeCreatePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type FileChangeWritePayload struct {
	Path             string `json:"path"`
	Text             string `json:"text"`
	ExpectedRevision string `json:"expected_revision"`
}

func DefaultFileChangeWritePayload() FileChangeWritePayload {
	return FileChangeWritePayload{
		Path:             "",
		Text:             "",
		ExpectedRevision: "",
	}
}
func (value *FileChangeWritePayload) UnmarshalJSON(data []byte) error {
	type plain FileChangeWritePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "text", "expected_revision"}, true); err != nil {
		return err
	}
	*value = FileChangeWritePayload(decoded)
	return nil
}
func (value FileChangeWritePayload) MarshalJSON() ([]byte, error) {
	type plain FileChangeWritePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type FileChangeEditPayload struct {
	Path             string     `json:"path"`
	ExpectedRevision string     `json:"expected_revision"`
	Edits            []FileEdit `json:"edits"`
}

func DefaultFileChangeEditPayload() FileChangeEditPayload {
	return FileChangeEditPayload{
		Path:             "",
		ExpectedRevision: "",
		Edits:            []FileEdit{},
	}
}
func (value *FileChangeEditPayload) UnmarshalJSON(data []byte) error {
	type plain FileChangeEditPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "expected_revision", "edits"}, true); err != nil {
		return err
	}
	*value = FileChangeEditPayload(decoded)
	return nil
}
func (value FileChangeEditPayload) MarshalJSON() ([]byte, error) {
	type plain FileChangeEditPayload
	encoded := plain(value)
	encoded.Edits = normalizeCollections(encoded.Edits)
	return json.Marshal(encoded)
}

type FileChangeMovePayload struct {
	Path             string `json:"path"`
	Destination      string `json:"destination"`
	ExpectedRevision string `json:"expected_revision"`
}

func DefaultFileChangeMovePayload() FileChangeMovePayload {
	return FileChangeMovePayload{
		Path:             "",
		Destination:      "",
		ExpectedRevision: "",
	}
}
func (value *FileChangeMovePayload) UnmarshalJSON(data []byte) error {
	type plain FileChangeMovePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "destination", "expected_revision"}, true); err != nil {
		return err
	}
	*value = FileChangeMovePayload(decoded)
	return nil
}
func (value FileChangeMovePayload) MarshalJSON() ([]byte, error) {
	type plain FileChangeMovePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type FileChangeDeletePayload struct {
	Path             string `json:"path"`
	ExpectedRevision string `json:"expected_revision"`
}

func DefaultFileChangeDeletePayload() FileChangeDeletePayload {
	return FileChangeDeletePayload{
		Path:             "",
		ExpectedRevision: "",
	}
}
func (value *FileChangeDeletePayload) UnmarshalJSON(data []byte) error {
	type plain FileChangeDeletePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "expected_revision"}, true); err != nil {
		return err
	}
	*value = FileChangeDeletePayload(decoded)
	return nil
}
func (value FileChangeDeletePayload) MarshalJSON() ([]byte, error) {
	type plain FileChangeDeletePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var fileChangeVariants = variantFactories{"create": func() any { return new(FileChangeCreatePayload) },
	"write":  func() any { return new(FileChangeWritePayload) },
	"edit":   func() any { return new(FileChangeEditPayload) },
	"move":   func() any { return new(FileChangeMovePayload) },
	"delete": func() any { return new(FileChangeDeletePayload) }}

func (value FileChange) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, fileChangeVariants)
}
func (value *FileChange) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", fileChangeVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type ChangeKind string

const (
	ChangeKindAdded    ChangeKind = "added"
	ChangeKindModified ChangeKind = "modified"
	ChangeKindRenamed  ChangeKind = "renamed"
	ChangeKindDeleted  ChangeKind = "deleted"
)

func (value ChangeKind) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ChangeKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ChangeKind(decoded) {
	case ChangeKindAdded, ChangeKindModified, ChangeKindRenamed, ChangeKindDeleted:
		*value = ChangeKind(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ChangeKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ChangeKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ChangesStatus string

const (
	ChangesStatusApplied  ChangesStatus = "applied"
	ChangesStatusPartial  ChangesStatus = "partial"
	ChangesStatusRejected ChangesStatus = "rejected"
)

func (value ChangesStatus) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ChangesStatus) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ChangesStatus(decoded) {
	case ChangesStatusApplied, ChangesStatusPartial, ChangesStatusRejected:
		*value = ChangesStatus(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ChangesStatus) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ChangesStatus) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ChangeStatus string

const (
	ChangeStatusApplied      ChangeStatus = "applied"
	ChangeStatusFailed       ChangeStatus = "failed"
	ChangeStatusUnknown      ChangeStatus = "unknown"
	ChangeStatusNotAttempted ChangeStatus = "not_attempted"
)

func (value ChangeStatus) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ChangeStatus) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ChangeStatus(decoded) {
	case ChangeStatusApplied, ChangeStatusFailed, ChangeStatusUnknown, ChangeStatusNotAttempted:
		*value = ChangeStatus(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ChangeStatus) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ChangeStatus) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type FileChanges struct {
	Status ChangesStatus `json:"status"`
	Files  []ChangedFile `json:"files"`
}

func DefaultFileChanges() FileChanges {
	return FileChanges{
		Status: ChangesStatus(""),
		Files:  []ChangedFile{},
	}
}
func (value *FileChanges) UnmarshalJSON(data []byte) error {
	type plain FileChanges
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"status", "files"}, false); err != nil {
		return err
	}
	*value = FileChanges(decoded)
	return nil
}
func (value FileChanges) MarshalJSON() ([]byte, error) {
	type plain FileChanges
	encoded := plain(value)
	encoded.Files = normalizeCollections(encoded.Files)
	return json.Marshal(encoded)
}

type ChangedFile struct {
	Path        string       `json:"path"`
	Destination *string      `json:"destination,omitempty"`
	Kind        ChangeKind   `json:"kind"`
	Status      ChangeStatus `json:"status"`
	Revision    *string      `json:"revision,omitempty"`
	Size        *uint64      `json:"size,omitempty"`
	Excerpt     *FileExcerpt `json:"excerpt,omitempty"`
	Error       *Fault       `json:"error,omitempty"`
}

func DefaultChangedFile() ChangedFile {
	return ChangedFile{
		Path:        "",
		Destination: nil,
		Kind:        ChangeKind(""),
		Status:      ChangeStatus(""),
		Revision:    nil,
		Size:        nil,
		Excerpt:     nil,
		Error:       nil,
	}
}
func (value *ChangedFile) UnmarshalJSON(data []byte) error {
	type plain ChangedFile
	decoded := plain{}
	decoded.Destination = nil
	decoded.Revision = nil
	decoded.Size = nil
	decoded.Excerpt = nil
	decoded.Error = nil
	if err := DecodeObject(data, &decoded, []string{"path", "kind", "status"}, false); err != nil {
		return err
	}
	*value = ChangedFile(decoded)
	return nil
}
func (value ChangedFile) MarshalJSON() ([]byte, error) {
	type plain ChangedFile
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ChangePreview struct {
	Files []PreviewFile `json:"files"`
}

func DefaultChangePreview() ChangePreview {
	return ChangePreview{
		Files: []PreviewFile{},
	}
}
func (value *ChangePreview) UnmarshalJSON(data []byte) error {
	type plain ChangePreview
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"files"}, false); err != nil {
		return err
	}
	*value = ChangePreview(decoded)
	return nil
}
func (value ChangePreview) MarshalJSON() ([]byte, error) {
	type plain ChangePreview
	encoded := plain(value)
	encoded.Files = normalizeCollections(encoded.Files)
	return json.Marshal(encoded)
}

type PreviewFile struct {
	Path        string     `json:"path"`
	Destination *string    `json:"destination,omitempty"`
	Kind        ChangeKind `json:"kind"`
	Additions   uint64     `json:"additions"`
	Deletions   uint64     `json:"deletions"`
	Diff        string     `json:"diff,omitempty"`
	Truncated   bool       `json:"truncated,omitempty"`
}

func DefaultPreviewFile() PreviewFile {
	return PreviewFile{
		Path:        "",
		Destination: nil,
		Kind:        ChangeKind(""),
		Additions:   0,
		Deletions:   0,
		Diff:        "",
		Truncated:   false,
	}
}
func (value *PreviewFile) UnmarshalJSON(data []byte) error {
	type plain PreviewFile
	decoded := plain{}
	decoded.Destination = nil
	decoded.Diff = ""
	decoded.Truncated = false
	if err := DecodeObject(data, &decoded, []string{"path", "kind", "additions", "deletions"}, false); err != nil {
		return err
	}
	*value = PreviewFile(decoded)
	return nil
}
func (value PreviewFile) MarshalJSON() ([]byte, error) {
	type plain PreviewFile
	encoded := plain(value)
	return json.Marshal(encoded)
}
