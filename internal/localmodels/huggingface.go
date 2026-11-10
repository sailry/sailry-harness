package localmodels

import (
	"context"
	"net/url"
	"strconv"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// repository is one Hub repository at a resolved revision.
type repository struct {
	source  protocol.ModelsSource
	name    string
	license string
	files   map[string]*file
}

// file is one repository file. Its checksum is a SHA-256, or a Git blob
// identity when the Hub publishes no SHA-256.
type file struct {
	source protocol.ModelsSource
	path   string
	bytes  uint64
	sha256 string
	blob   string
}

type candidate struct {
	repository string
	name       string
}

type listing struct {
	items   []candidate
	next    *string
	partial bool
}

func item(repository string, name *string, hub protocol.ModelsHub) (candidate, bool) {
	if ValidateSource(protocol.ModelsSource{Hub: hub, Repository: repository, Revision: "main"}) != nil {
		return candidate{}, false
	}
	label := repository
	if name != nil {
		label = *name
	}
	if label == "" || len(label) > 256 || strings.Contains(label, "\x00") {
		return candidate{}, false
	}
	return candidate{repository: repository, name: label}, true
}

func huggingfaceSearch(ctx context.Context, hub *catalog, request *protocol.ModelsSearch) (listing, error) {
	address := hub.url("api", "models")
	var pairs []string
	add := func(key, value string) { pairs = append(pairs, url.QueryEscape(key)+"="+url.QueryEscape(value)) }
	if request.Kind == protocol.ModelsKindDecision {
		add("filter", "gguf")
	}
	if strings.TrimSpace(request.Query) == "" {
		if request.Kind != protocol.ModelsKindDecision {
			add("filter", "onnx")
		}
		add("sort", "downloads")
		add("direction", "-1")
		switch request.Kind {
		case protocol.ModelsKindSpeech:
			add("search", "sensevoice")
		case protocol.ModelsKindOcr:
			add("search", "paddleocr")
		}
	} else {
		add("search", strings.TrimSpace(request.Query))
	}
	add("limit", strconv.Itoa(pageSize))
	// Compatible exports can lack pipeline metadata.
	// Compatibility is established by resolution and installation.
	if request.Cursor != nil {
		add("cursor", *request.Cursor)
	}
	address.RawQuery = strings.Join(pairs, "&")
	value, headers, err := hub.json(ctx, address)
	if err != nil {
		return listing{}, err
	}
	rows, ok := value.([]any)
	if !ok {
		return listing{}, invalid("Hugging Face model listing is invalid")
	}
	if len(rows) > pageSize {
		return listing{}, invalid("Hugging Face listing exceeds its requested page size")
	}
	result := listing{}
	for _, row := range rows {
		if id, ok := text(row, "id"); ok {
			if found, ok := item(id, nil, protocol.ModelsHubHuggingFace); ok {
				result.items = append(result.items, found)
			}
		}
	}
	for _, link := range headers.Values("Link") {
		for part := range strings.SplitSeq(link, ",") {
			fields := strings.Split(strings.TrimSpace(part), ";")
			following := false
			for _, attribute := range fields[1:] {
				following = following || strings.TrimSpace(attribute) == `rel="next"`
			}
			if !following {
				continue
			}
			target := strings.TrimSpace(fields[0])
			if !strings.HasPrefix(target, "<") || !strings.HasSuffix(target, ">") {
				return listing{}, invalid("Hugging Face pagination is invalid")
			}
			next, err := url.Parse(target[1 : len(target)-1])
			if err != nil || !next.IsAbs() {
				return listing{}, invalid("Hugging Face pagination is invalid")
			}
			if next.Scheme != address.Scheme || next.Host != address.Host || next.EscapedPath() != address.EscapedPath() {
				return listing{}, invalid("Hugging Face pagination changed its endpoint")
			}
			cursor := next.Query().Get("cursor")
			if cursor == "" || len(cursor) > 8192 {
				return listing{}, invalid("Hugging Face pagination cursor is invalid")
			}
			result.next = &cursor
		}
	}
	result.partial = len(result.items) != len(rows)
	return result, nil
}

func huggingfaceVersions(ctx context.Context, hub *catalog, name string) ([]protocol.ModelsVersion, error) {
	value, _, err := hub.json(ctx, hub.repository([]string{"api", "models"}, name, "refs"))
	if err != nil {
		return nil, err
	}
	versions := []protocol.ModelsVersion{}
	for _, key := range []string{"branches", "tags"} {
		rows, ok := list(value, key)
		if !ok {
			return nil, invalid("Hugging Face version listing is invalid")
		}
		for _, row := range rows {
			label, named := text(row, "name")
			revision, pinned := text(row, "targetCommit")
			if !named || !pinned || !commit(revision) || label == "" || len(label) > 256 || strings.Contains(label, "\x00") {
				return nil, invalid("Hugging Face version is not pinned")
			}
			versions = append(versions, protocol.ModelsVersion{Name: label, Revision: strings.ToLower(revision)})
		}
	}
	return versions, nil
}

func huggingfaceRepository(ctx context.Context, hub *catalog, source protocol.ModelsSource) (*repository, error) {
	address := hub.repository([]string{"api", "models"}, source.Repository, "revision", source.Revision)
	address.RawQuery = "blobs=true"
	value, _, err := hub.json(ctx, address)
	if err != nil {
		return nil, err
	}
	revision, ok := text(value, "sha")
	if !ok || !commit(revision) {
		return nil, invalid("Hugging Face model revision is not pinned")
	}
	if commit(source.Revision) && !strings.EqualFold(source.Revision, revision) {
		return nil, newFault(protocol.ErrorCodeRevisionConflict, "Hugging Face returned a different model revision")
	}
	private, _ := flag(value, "private")
	gated := false
	if raw, present := field(value, "gated"); present {
		switch state := raw.(type) {
		case bool:
			gated = state
		case string:
			gated = state != "false"
		}
	}
	if private || gated {
		return nil, newFault(protocol.ErrorCodePermissionDenied, "model repository requires authentication")
	}
	pinned := source
	pinned.Revision = strings.ToLower(revision)
	rows, ok := list(value, "siblings")
	if !ok {
		return nil, invalid("Hugging Face file listing is invalid")
	}
	files := map[string]*file{}
	for _, row := range rows {
		path, ok := text(row, "rfilename")
		if !ok {
			return nil, invalid("Hugging Face file path is invalid")
		}
		bytes, ok := count(row, "size")
		if !ok {
			return nil, invalid("Hugging Face file size is unavailable")
		}
		lfs, _ := field(row, "lfs")
		sha256, hashed := text(lfs, "sha256")
		sha256 = strings.ToLower(sha256)
		if size, ok := count(lfs, "size"); hashed && (!ok || size != bytes) {
			return nil, newFault(protocol.ErrorCodeRevisionConflict, "Hugging Face file sizes differ in metadata")
		}
		blob, _ := text(row, "blobId")
		blob = strings.ToLower(blob)
		if hashed && !digest(sha256) || !hashed && !commit(blob) {
			return nil, invalid("Hugging Face file checksum is unavailable")
		}
		if _, duplicate := files[path]; duplicate {
			return nil, invalid("Hugging Face file listing contains duplicates")
		}
		files[path] = &file{source: pinned, path: path, bytes: bytes, sha256: sha256, blob: blob}
	}
	card, _ := field(value, "cardData")
	license, _ := text(card, "license")
	return &repository{source: pinned, name: source.Repository, license: license, files: files}, nil
}
