package localmodels

import (
	"context"
	"maps"
	"net/url"
	"slices"
	"strconv"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const resultLimit = 3000

var modelscopeModels = []string{"openapi", "v1", "models"}

func modelscopeSearch(ctx context.Context, hub *catalog, request *protocol.ModelsSearch) (listing, error) {
	page := uint64(1)
	if request.Cursor != nil {
		parsed, err := strconv.ParseUint(*request.Cursor, 10, 32)
		if err != nil {
			return listing{}, invalid("ModelScope pagination cursor is invalid")
		}
		page = parsed
	}
	if page == 0 || page > resultLimit/pageSize {
		return listing{}, invalid("ModelScope pagination exceeds its result limit")
	}
	search := strings.TrimSpace(request.Query)
	if search == "" {
		search = map[protocol.ModelsKind]string{
			protocol.ModelsKindEmbedding: "onnx",
			protocol.ModelsKindSpeech:    "sensevoice",
			protocol.ModelsKindOcr:       "oar-ocr",
			protocol.ModelsKindDecision:  "gguf",
		}[request.Kind]
	}
	address := hub.url(modelscopeModels...)
	address.RawQuery = "search=" + url.QueryEscape(search) + "&page_number=" + strconv.FormatUint(page, 10) + "&page_size=" + strconv.Itoa(pageSize)
	// Task labels vary across ONNX exports and can be absent for Model2Vec.
	// Resolve the selected candidate against the supported runtime format.
	value, _, err := hub.json(ctx, address)
	if err != nil {
		return listing{}, err
	}
	data, err := modelscopeData(value)
	if err != nil {
		return listing{}, err
	}
	rows, ok := list(data, "models")
	if !ok {
		return listing{}, invalid("ModelScope model listing is invalid")
	}
	total, ok := count(data, "total_count")
	if !ok {
		return listing{}, invalid("ModelScope result count is unavailable")
	}
	number, numbered := count(data, "page_number")
	size, sized := count(data, "page_size")
	if !numbered || number != page || !sized || size != pageSize {
		return listing{}, invalid("ModelScope returned a different result page")
	}
	if len(rows) > pageSize {
		return listing{}, invalid("ModelScope listing exceeds its requested page size")
	}
	result := listing{}
	for _, row := range rows {
		id, ok := text(row, "id")
		if !ok {
			continue
		}
		var name *string
		if display, ok := text(row, "display_name"); ok {
			name = &display
		}
		if found, ok := item(id, name, protocol.ModelsHubModelScope); ok {
			result.items = append(result.items, found)
		}
	}
	more := page*pageSize < total
	capped := (page+1)*pageSize > resultLimit
	result.partial = len(result.items) != len(rows) || more && capped
	if more && !capped {
		next := strconv.FormatUint(page+1, 10)
		result.next = &next
	}
	return result, nil
}

// modelscopeHead resolves the default branch of a repository.
func modelscopeHead(ctx context.Context, hub *catalog, name string) (protocol.ModelsSource, error) {
	source := protocol.ModelsSource{Hub: protocol.ModelsHubModelScope, Repository: name}
	value, _, err := hub.json(ctx, hub.repository(modelscopeModels, name))
	if err != nil {
		return source, err
	}
	info, err := modelscopeData(value)
	if err != nil {
		return source, err
	}
	if branch, ok := text(info, "default_branch"); ok {
		source.Revision = branch
	} else {
		revisions, err := modelscopeRevisions(ctx, hub, name)
		if err != nil {
			return source, err
		}
		if len(revisions.branches) != 1 {
			return source, invalid("model default branch is unavailable")
		}
		source.Revision = revisions.branches[0]
	}
	return source, ValidateSource(source)
}

func modelscopeVersions(ctx context.Context, hub *catalog, name string) ([]protocol.ModelsVersion, error) {
	refs, err := readRefs(ctx, hub, name)
	if err == nil {
		return refs.versions(), nil
	}
	if code(err) == protocol.ErrorCodeCancelled {
		return nil, err
	}
	revisions, err := modelscopeRevisions(ctx, hub, name)
	if err != nil {
		return nil, err
	}
	versions := []protocol.ModelsVersion{}
	for _, label := range slices.Concat(revisions.branches, revisions.tags) {
		versions = append(versions, protocol.ModelsVersion{Name: label, Revision: label})
	}
	return versions, nil
}

func modelscopeRepository(ctx context.Context, hub *catalog, source protocol.ModelsSource) (*repository, error) {
	value, _, err := hub.json(ctx, hub.repository(modelscopeModels, source.Repository))
	if err != nil {
		return nil, err
	}
	info, err := modelscopeData(value)
	if err != nil {
		return nil, err
	}
	private, _ := flag(info, "private")
	gated, _ := flag(info, "gated")
	if private || gated {
		return nil, newFault(protocol.ErrorCodePermissionDenied, "model repository requires authentication")
	}
	pinned := source
	snapshot := false
	if commit(source.Revision) {
		pinned.Revision = strings.ToLower(pinned.Revision)
	} else if refs, err := readRefs(ctx, hub, source.Repository); err == nil {
		revision, found := refs.resolve(source.Revision)
		if !found {
			return nil, newFault(protocol.ErrorCodeNotFound, "ModelScope branch or tag was not found")
		}
		pinned.Revision = revision
	} else if code(err) == protocol.ErrorCodeCancelled {
		return nil, err
	} else {
		revisions, err := modelscopeRevisions(ctx, hub, source.Repository)
		if err != nil {
			return nil, err
		}
		branch, known := text(info, "default_branch")
		if !known && len(revisions.branches) == 1 {
			branch, known = revisions.branches[0], true
		}
		if !known || branch != source.Revision || !slices.Contains(revisions.branches, source.Revision) || slices.Contains(revisions.tags, source.Revision) {
			return nil, newFault(protocol.ErrorCodeUnavailable, "ModelScope could not pin this branch or tag; select a full commit")
		}
		// The visible source remains the selected branch. Each asset pins
		// its own immutable file revision from this one current tree.
		snapshot = true
	}
	address := hub.repository([]string{"api", "v1", "models"}, source.Repository, "repo", "files")
	address.RawQuery = "Revision=" + url.QueryEscape(pinned.Revision) + "&Recursive=True"
	value, _, err = hub.json(ctx, address)
	if err != nil {
		return nil, err
	}
	data, err := modelscopeData(value)
	if err != nil {
		return nil, err
	}
	rows, ok := list(data, "Files")
	if !ok {
		return nil, invalid("ModelScope file listing is invalid")
	}
	if len(rows) >= resultLimit {
		return nil, invalid("ModelScope file listing may be truncated")
	}
	files := map[string]*file{}
	for _, row := range rows {
		if kind, _ := text(row, "Type"); kind == "tree" {
			continue
		}
		path, ok := text(row, "Path")
		if !ok {
			return nil, invalid("ModelScope file path is invalid")
		}
		bytes, ok := count(row, "Size")
		if !ok {
			return nil, invalid("ModelScope file size is unavailable")
		}
		sha256, ok := text(row, "Sha256")
		if !ok || !digest(sha256) {
			return nil, invalid("ModelScope file checksum is unavailable")
		}
		origin := pinned
		if snapshot {
			revision, ok := text(row, "Revision")
			if !ok || !commit(revision) {
				return nil, invalid("ModelScope file revision is not pinned")
			}
			origin.Revision = strings.ToLower(revision)
		}
		if _, duplicate := files[path]; duplicate {
			return nil, invalid("ModelScope file listing contains duplicates")
		}
		files[path] = &file{source: origin, path: path, bytes: bytes, sha256: strings.ToLower(sha256)}
	}
	name, ok := text(info, "display_name")
	if !ok {
		name = source.Repository
	}
	license, _ := text(info, "license")
	return &repository{source: pinned, name: name, license: license, files: files}, nil
}

type revisions struct {
	branches []string
	tags     []string
}

func modelscopeRevisions(ctx context.Context, hub *catalog, name string) (revisions, error) {
	value, _, err := hub.json(ctx, hub.repository([]string{"api", "v1", "models"}, name, "revisions"))
	if err != nil {
		return revisions{}, err
	}
	data, err := modelscopeData(value)
	if err != nil {
		return revisions{}, err
	}
	names, ok := field(data, "RevisionMap")
	if !ok {
		return revisions{}, invalid("ModelScope version listing is invalid")
	}
	read := func(key string) ([]string, error) {
		rows, ok := list(names, key)
		if !ok {
			return nil, invalid("ModelScope version listing is invalid")
		}
		result := make([]string, 0, len(rows))
		for _, row := range rows {
			label, ok := text(row, "Revision")
			if !ok {
				return nil, invalid("ModelScope version name is invalid")
			}
			if err := ValidateSource(protocol.ModelsSource{Hub: protocol.ModelsHubModelScope, Repository: name, Revision: label}); err != nil {
				return nil, err
			}
			result = append(result, label)
		}
		return result, nil
	}
	branches, err := read("Branches")
	if err != nil {
		return revisions{}, err
	}
	tags, err := read("Tags")
	return revisions{branches: branches, tags: tags}, err
}

func modelscopeData(value any) (any, error) {
	for _, key := range []string{"success", "Success"} {
		if succeeded, present := flag(value, key); present && !succeeded {
			return nil, newFault(protocol.ErrorCodeUnavailable, "ModelScope rejected the catalog request")
		}
	}
	data, present := field(value, "data")
	if !present {
		data, _ = field(value, "Data")
	}
	if _, object := data.(map[string]any); !object {
		return nil, invalid("ModelScope returned invalid model metadata")
	}
	return data, nil
}

// refs is a Git smart-HTTP ref advertisement: branch and tag commits.
type refs struct {
	heads map[string]string
	tags  map[string]string
}

func readRefs(ctx context.Context, hub *catalog, name string) (*refs, error) {
	address := hub.repository(nil, name+".git", "info", "refs")
	address.RawQuery = "service=git-upload-pack"
	body, _, err := hub.get(ctx, address, metadataBytes)
	if err != nil {
		return nil, err
	}
	return parseRefs(body)
}

func parseRefs(body []byte) (*refs, error) {
	bad := invalid("ModelScope did not return a Git ref advertisement")
	heads, tags, peeled := map[string]string{}, map[string]string{}, map[string]string{}
	insert := func(target map[string]string, name, revision string) bool {
		if _, duplicate := target[name]; name == "" || len(name) > 256 || strings.Contains(name, "\x00") || duplicate {
			return false
		}
		target[name] = revision
		return true
	}
	service := false
	for len(body) > 0 {
		if len(body) < 4 {
			return nil, bad
		}
		length, err := strconv.ParseUint(string(body[:4]), 16, 32)
		if err != nil {
			return nil, bad
		}
		body = body[4:]
		if length == 0 {
			continue
		}
		if length < 4 || int(length-4) > len(body) {
			return nil, bad
		}
		line := string(body[:length-4])
		body = body[length-4:]
		if !service {
			if line != "# service=git-upload-pack\n" {
				return nil, bad
			}
			service = true
			continue
		}
		if line == "version 1\n" {
			continue
		}
		line, _, _ = strings.Cut(line, "\x00")
		revision, name, found := strings.Cut(strings.TrimRight(line, " \t\r\n"), " ")
		if !found || !commit(revision) {
			return nil, bad
		}
		revision = strings.ToLower(revision)
		accepted := true
		if head, ok := strings.CutPrefix(name, "refs/heads/"); ok {
			accepted = insert(heads, head, revision)
		} else if tag, ok := strings.CutPrefix(name, "refs/tags/"); ok {
			if target, ok := strings.CutSuffix(tag, "^{}"); ok {
				accepted = insert(peeled, target, revision)
			} else {
				accepted = insert(tags, tag, revision)
			}
		}
		if !accepted {
			return nil, bad
		}
	}
	if !service || len(heads) == 0 && len(tags) == 0 {
		return nil, bad
	}
	for name, revision := range peeled {
		if _, known := tags[name]; !known {
			return nil, bad
		}
		tags[name] = revision
	}
	return &refs{heads: heads, tags: tags}, nil
}

func (refs *refs) resolve(name string) (string, bool) {
	if head, ok := strings.CutPrefix(name, "refs/heads/"); ok {
		revision, found := refs.heads[head]
		return revision, found
	}
	if tag, ok := strings.CutPrefix(name, "refs/tags/"); ok {
		revision, found := refs.tags[tag]
		return revision, found
	}
	head, branch := refs.heads[name]
	tag, tagged := refs.tags[name]
	switch {
	case branch && tagged && head != tag:
		return "", false
	case branch:
		return head, true
	}
	return tag, tagged
}

func (refs *refs) versions() []protocol.ModelsVersion {
	versions := []protocol.ModelsVersion{}
	for _, group := range []map[string]string{refs.heads, refs.tags} {
		for _, name := range slices.Sorted(maps.Keys(group)) {
			versions = append(versions, protocol.ModelsVersion{Name: name, Revision: group[name]})
		}
	}
	return versions
}
