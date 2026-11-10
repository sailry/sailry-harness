package plugins

import (
	"archive/zip"
	"bytes"
	"context"
	"crypto/ed25519"
	"crypto/md5"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"slices"
	"strconv"
	"strings"
	"time"
	"unicode"

	"github.com/Masterminds/semver/v3"
	"github.com/sailry/sailry-harness/internal/protocol"
	"go.yaml.in/yaml/v3"
)

const SkillRegistryURL = "https://api.skillhub.cn"

func registrySegment(value string) bool {
	if value == "" || value == "." || value == ".." || len(value) > 128 {
		return false
	}
	for _, character := range []byte(value) {
		if !(character >= 'a' && character <= 'z' || character >= 'A' && character <= 'Z' || character >= '0' && character <= '9' || character == '-' || character == '_' || character == '.') {
			return false
		}
	}
	return true
}

func ValidateRegistrySource(source protocol.PluginSkillsRegistrySource) error {
	if !registrySegment(source.Slug) || source.Namespace != nil && !registrySegment(*source.Namespace) {
		return packageFault("invalid Skillhub package identity or signature")
	}
	if source.Version != nil {
		if _, err := semver.StrictNewVersion(*source.Version); err != nil {
			return packageFault("invalid Skillhub package identity or signature")
		}
	}
	return nil
}

func RegistrySourceURL(input string) (*protocol.PluginSkillsRegistrySource, error) {
	address, err := url.Parse(input)
	if err != nil || address.Hostname() != "skillhub.cn" && address.Hostname() != "www.skillhub.cn" {
		return nil, nil
	}
	parts := strings.Split(strings.Trim(address.Path, "/"), "/")
	if len(input) > 2048 || strings.Contains(input, "\\") || strings.ContainsFunc(input, unicode.IsControl) ||
		address.Scheme != "https" || address.User != nil || address.Port() != "" || address.Fragment != "" ||
		len(parts) != 2 && len(parts) != 3 || parts[0] != "skills" {
		return nil, packageFault("invalid Skillhub package identity or signature")
	}
	source := protocol.PluginSkillsRegistrySource{Slug: parts[len(parts)-1]}
	if len(parts) == 3 {
		source.Namespace = &parts[1]
	}
	for key, values := range address.Query() {
		if key != "version" || len(values) != 1 {
			return nil, packageFault("invalid Skillhub package identity or signature")
		}
		source.Version = &values[0]
	}
	if err := ValidateRegistrySource(source); err != nil {
		return nil, err
	}
	return &source, nil
}

type registryEnvelope struct {
	Signed      bool   `json:"signed"`
	KeyID       string `json:"key_id"`
	Payload     string `json:"payload"`
	Signature   string `json:"signature"`
	ContentHash string `json:"content_hash"`
	HashVersion uint32 `json:"hash_version"`
}

type registryKeys struct {
	Keys []struct {
		KeyID     string `json:"key_id"`
		Algorithm string `json:"algorithm"`
		PublicKey string `json:"public_key_raw_b64"`
		Status    string `json:"status"`
		Issuer    string `json:"issuer"`
	} `json:"keys"`
}

type registryPayload struct {
	Version      uint32 `json:"v"`
	Issuer       string `json:"issuer"`
	Slug         string `json:"skill_slug"`
	SkillVersion string `json:"skill_version"`
	VersionID    uint64 `json:"version_id"`
	PublisherID  uint64 `json:"publisher_id"`
	ContentHash  string `json:"content_hash"`
	PackageMD5   string `json:"package_md5"`
	FileCount    int    `json:"file_count"`
}

func verifyRegistrySignature(envelope registryEnvelope, keys registryKeys, slug, version string) (registryPayload, error) {
	invalid := func() (registryPayload, error) {
		return registryPayload{}, packageFault("invalid Skillhub package identity or signature")
	}
	if !envelope.Signed || envelope.HashVersion != 1 || len(envelope.Payload) > 64*1024 {
		return invalid()
	}
	found := -1
	for index, key := range keys.Keys {
		if key.KeyID == envelope.KeyID {
			if found >= 0 || key.Algorithm != "Ed25519" || key.Status != "active" || key.Issuer != "skillhub.cn" {
				return invalid()
			}
			found = index
		}
	}
	if found < 0 {
		return invalid()
	}
	key, err := base64.StdEncoding.DecodeString(keys.Keys[found].PublicKey)
	if err != nil || len(key) != ed25519.PublicKeySize {
		return invalid()
	}
	signature, err := base64.StdEncoding.DecodeString(envelope.Signature)
	if err != nil || len(signature) != ed25519.SignatureSize || !ed25519.Verify(key, []byte(envelope.Payload), signature) {
		return invalid()
	}
	var payload registryPayload
	if json.Unmarshal([]byte(envelope.Payload), &payload) != nil || payload.Version != 1 || payload.Issuer != "skillhub.cn" ||
		payload.Slug != slug || payload.SkillVersion != version || payload.VersionID == 0 || payload.PublisherID == 0 ||
		payload.FileCount <= 0 || payload.FileCount > 10000 || !lowerHex(payload.ContentHash, 32) || !lowerHex(payload.PackageMD5, 16) ||
		payload.ContentHash != envelope.ContentHash {
		return invalid()
	}
	return payload, nil
}

func lowerHex(value string, size int) bool {
	data, err := hex.DecodeString(value)
	return err == nil && len(data) == size && hex.EncodeToString(data) == value
}

func excludedRegistryFile(name string) bool {
	for _, part := range strings.Split(name, "/") {
		if part == "__MACOSX" || part == ".DS_Store" || strings.EqualFold(part, "Thumbs.db") || strings.HasPrefix(part, "._") {
			return true
		}
	}
	return false
}

func verifyRegistryArchive(ctx context.Context, data []byte, resolved protocol.PluginSkillsRegistryResolved, payload registryPayload) (protocol.PluginSkillsRegistryResolved, map[string][]byte, error) {
	if fmt.Sprintf("%x", md5.Sum(data)) != payload.PackageMD5 {
		return resolved, nil, packageFault("invalid Skillhub package identity or signature")
	}
	resolved.ArchiveSha256 = fmt.Sprintf("%x", sha256.Sum256(data))
	archive, err := zip.NewReader(bytes.NewReader(data), int64(len(data)))
	if err != nil || len(archive.File) > 10000 {
		return resolved, nil, packageFault("invalid Skillhub archive")
	}
	files, hashes, names := map[string][]byte{}, map[string]string{}, map[string]bool{}
	expanded := uint64(0)
	for _, file := range archive.File {
		if err := ctx.Err(); err != nil {
			return resolved, nil, err
		}
		name := strings.TrimSuffix(file.Name, "/")
		if !resourcePath(name) || names[name] || file.Flags&1 != 0 ||
			!file.FileInfo().IsDir() && !file.Mode().IsRegular() {
			return resolved, nil, packageFault("invalid Skillhub archive entry")
		}
		names[name] = true
		if file.FileInfo().IsDir() {
			continue
		}
		if file.UncompressedSize64 > MaxPackageBytes-expanded {
			return resolved, nil, packageFault("Skillhub archive exceeds its expanded size limit")
		}
		reader, err := file.Open()
		if err != nil {
			return resolved, nil, err
		}
		body, err := io.ReadAll(io.LimitReader(reader, int64(file.UncompressedSize64)+1))
		closeErr := reader.Close()
		if err != nil {
			return resolved, nil, err
		}
		if closeErr != nil {
			return resolved, nil, closeErr
		}
		if uint64(len(body)) != file.UncompressedSize64 {
			return resolved, nil, packageFault("Skillhub archive size differs from its declaration")
		}
		expanded += uint64(len(body))
		files[name] = body
		if !excludedRegistryFile(name) {
			hashes[name] = fmt.Sprintf("%x", sha256.Sum256(body))
		}
	}
	ordered := []string{}
	for name := range hashes {
		ordered = append(ordered, name)
	}
	slices.Sort(ordered)
	match := false
	for _, includeMetadata := range []bool{false, true} {
		hasher, count := sha256.New(), 0
		for _, name := range ordered {
			if name == "_meta.json" && !includeMetadata {
				continue
			}
			fmt.Fprintf(hasher, "%s:%s\n", name, hashes[name])
			count++
		}
		match = match || fmt.Sprintf("%x", hasher.Sum(nil)) == payload.ContentHash && count == payload.FileCount
	}
	if !match {
		return resolved, nil, packageFault("Skillhub archive content differs from its signed tree")
	}
	return resolved, files, nil
}

func (manager *Manager) registryEndpoint() string {
	if manager.RegistryURL != "" {
		return strings.TrimSuffix(manager.RegistryURL, "/")
	}
	return SkillRegistryURL
}

func (manager *Manager) registryDownload(ctx context.Context, endpoint string, limit int64) ([]byte, error) {
	client := http.Client{Timeout: 30 * time.Second}
	if manager.Client != nil {
		client = *manager.Client
	}
	allowed := func(address *url.URL) bool {
		if address.User != nil || address.Fragment != "" {
			return false
		}
		if manager.RegistryURL != "" {
			fixture, err := url.Parse(manager.RegistryURL)
			if err == nil && fixture.Scheme == address.Scheme && fixture.Host == address.Host {
				return true
			}
		}
		return address.Scheme == "https" && (address.Port() == "" || address.Port() == "443") &&
			(address.Hostname() == "api.skillhub.cn" || strings.HasSuffix(address.Hostname(), ".myqcloud.com"))
	}
	client.CheckRedirect = func(request *http.Request, previous []*http.Request) error {
		if len(previous) > 5 || !allowed(request.URL) {
			return packageFault("unsupported Skillhub download redirect")
		}
		return nil
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil || !allowed(request.URL) {
		return nil, packageFault("invalid Skillhub download URL")
	}
	request.Header.Set("User-Agent", "Sailry")
	response, err := client.Do(request)
	if err != nil {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Skillhub source is unavailable"}
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Skillhub source is unavailable"}
	}
	data, err := io.ReadAll(io.LimitReader(response.Body, limit+1))
	if err != nil {
		return nil, err
	}
	if int64(len(data)) > limit {
		return nil, packageFault("Skillhub download exceeds its size limit")
	}
	return data, nil
}

func (manager *Manager) acquireRegistry(ctx context.Context, source protocol.PluginSkillsRegistrySource, expected *protocol.PluginSkillsRegistryResolved) (protocol.PluginSkillsRegistryResolved, map[string][]byte, error) {
	if err := ValidateRegistrySource(source); err != nil {
		return protocol.PluginSkillsRegistryResolved{}, nil, err
	}
	ctx, cancel := context.WithTimeout(ctx, 60*time.Second)
	defer cancel()
	parameters := url.Values{}
	if source.Namespace != nil {
		parameters.Set("namespace", *source.Namespace)
	}
	var detail struct {
		Namespace struct {
			Handle string `json:"handle"`
		} `json:"namespace"`
		Skill struct {
			Slug string `json:"slug"`
		} `json:"skill"`
		Latest struct {
			Version string `json:"version"`
		} `json:"latestVersion"`
	}
	root := manager.registryEndpoint()
	if err := manager.catalogJSON(ctx, root+"/api/v1/skills/"+url.PathEscape(source.Slug)+"?"+parameters.Encode(), &detail); err != nil {
		return protocol.PluginSkillsRegistryResolved{}, nil, err
	}
	if !registrySegment(detail.Namespace.Handle) || detail.Skill.Slug != source.Slug || source.Namespace != nil && *source.Namespace != detail.Namespace.Handle {
		return protocol.PluginSkillsRegistryResolved{}, nil, packageFault("invalid Skillhub package identity or signature")
	}
	version := detail.Latest.Version
	if source.Version != nil {
		version = *source.Version
	}
	if _, err := semver.StrictNewVersion(version); err != nil {
		return protocol.PluginSkillsRegistryResolved{}, nil, packageFault("invalid Skillhub package version")
	}
	parameters.Set("namespace", detail.Namespace.Handle)
	var envelope registryEnvelope
	var keys registryKeys
	if err := manager.catalogJSON(ctx, root+"/api/v1/open/skills/"+url.PathEscape(source.Slug)+"/versions/"+url.PathEscape(version)+"/signature?"+parameters.Encode(), &envelope); err != nil {
		return protocol.PluginSkillsRegistryResolved{}, nil, err
	}
	if err := manager.catalogJSON(ctx, root+"/api/v1/open/platform/keys", &keys); err != nil {
		return protocol.PluginSkillsRegistryResolved{}, nil, err
	}
	payload, err := verifyRegistrySignature(envelope, keys, source.Slug, version)
	if err != nil {
		return protocol.PluginSkillsRegistryResolved{}, nil, err
	}
	parameters.Set("slug", source.Slug)
	parameters.Set("version", version)
	data, err := manager.registryDownload(ctx, root+"/api/v1/download?"+parameters.Encode(), MaxPackageBytes)
	if err != nil {
		return protocol.PluginSkillsRegistryResolved{}, nil, err
	}
	resolved := protocol.PluginSkillsRegistryResolved{Namespace: detail.Namespace.Handle, Slug: source.Slug, Version: version,
		VersionID: strconv.FormatUint(payload.VersionID, 10), ContentHash: payload.ContentHash, PackageMd5: payload.PackageMD5}
	resolved, files, err := verifyRegistryArchive(ctx, data, resolved, payload)
	if err != nil {
		return resolved, nil, err
	}
	if expected != nil && *expected != resolved {
		return resolved, nil, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "Skillhub package changed; discover it again"}
	}
	manager.repositoryMu.Lock()
	manager.registryArchives[resolved.ArchiveSha256] = data
	manager.repositoryMu.Unlock()
	return resolved, files, nil
}

func registryPackageName(source protocol.PluginSkillsRegistryResolved) string {
	digest := sha256.Sum256([]byte("skillhub.cn/" + source.Namespace + "/" + source.Slug))
	return fmt.Sprintf("skillhub-%x", digest[:12])
}

func normalizeRegistrySkill(data []byte, source protocol.PluginSkillsRegistryResolved) ([]byte, error) {
	if len(data) > MaxSkillDocumentBytes {
		return nil, packageFault("registry skill document exceeds its size limit")
	}
	text := strings.TrimLeft(strings.TrimPrefix(string(data), "\ufeff"), "\r\n \t")
	lines := strings.Split(strings.ReplaceAll(text, "\r\n", "\n"), "\n")
	if len(lines) == 0 || lines[0] != "---" {
		return nil, packageFault("registry skill requires YAML frontmatter")
	}
	end := -1
	for index := 1; index < len(lines); index++ {
		if lines[index] == "---" || lines[index] == "..." {
			end = index
			break
		}
	}
	if end < 0 {
		return nil, packageFault("registry skill frontmatter is unterminated")
	}
	var document yaml.Node
	count := 0
	header := []byte(strings.Join(lines[1:end], "\n"))
	if len(header) > 16*1024 || yaml.Unmarshal(header, &document) != nil || !validateYAML(&document, 0, &count) {
		return nil, packageFault("invalid registry skill frontmatter")
	}
	var fields map[string]any
	if document.Decode(&fields) != nil || fields == nil {
		return nil, packageFault("invalid registry skill metadata")
	}
	title, ok := fields["name"].(string)
	if !ok || strings.TrimSpace(title) == "" || len(title) > 256 || strings.ContainsFunc(title, unicode.IsControl) ||
		skillName(title) && title != source.Slug || !skillName(source.Slug) {
		return nil, packageFault("registry skill identity does not match")
	}
	if slug, exists := fields["slug"]; exists && slug != source.Slug {
		return nil, packageFault("registry skill identity does not match")
	}
	fields["name"] = source.Slug
	metadata, exists := fields["metadata"]
	if !exists {
		metadata = map[string]any{}
		fields["metadata"] = metadata
	}
	labels, ok := metadata.(map[string]any)
	if !ok {
		return nil, packageFault("invalid registry skill metadata")
	}
	for key, value := range labels {
		if _, ok := value.(string); !ok {
			if key == "display-name" || strings.HasPrefix(key, "display-name.") {
				return nil, packageFault("invalid localized registry skill metadata")
			}
			data, err := json.Marshal(value)
			if err != nil {
				return nil, err
			}
			labels[key] = string(data)
		}
	}
	if !skillName(title) {
		if _, exists := labels["display-name"]; !exists {
			labels["display-name"] = title
		}
	}
	encoded, err := json.Marshal(fields)
	if err != nil {
		return nil, err
	}
	result := []byte("---\n" + string(encoded) + "\n---\n" + strings.Join(lines[end+1:], "\n"))
	if _, err := ParseSkill(result, source.Slug); err != nil {
		return nil, err
	}
	return result, nil
}

func (manager *Manager) DiscoverRegistry(ctx context.Context, source protocol.PluginSkillsRegistrySource) (protocol.PluginSkillsRegistryDiscovery, error) {
	resolved, files, err := manager.acquireRegistry(ctx, source, nil)
	if err != nil {
		return protocol.PluginSkillsRegistryDiscovery{}, err
	}
	document, err := normalizeRegistrySkill(files["SKILL.md"], resolved)
	if err != nil {
		return protocol.PluginSkillsRegistryDiscovery{}, err
	}
	skill, err := ParseSkill(document, resolved.Slug)
	if err != nil {
		return protocol.PluginSkillsRegistryDiscovery{}, err
	}
	return protocol.PluginSkillsRegistryDiscovery{Source: resolved, Skills: []protocol.PluginSkillsCandidate{{Name: registryPackageName(resolved), Path: "", Skill: skill}}}, nil
}

func (manager *Manager) RegistryDocument(ctx context.Context, source protocol.PluginSkillsRegistryResolved) (protocol.PluginSkillsDocument, error) {
	resolved, files, err := manager.acquireRegistry(ctx, protocol.PluginSkillsRegistrySource{Namespace: &source.Namespace, Slug: source.Slug, Version: &source.Version}, &source)
	if err != nil {
		return protocol.PluginSkillsDocument{}, err
	}
	document, err := normalizeRegistrySkill(files["SKILL.md"], resolved)
	if err != nil {
		return protocol.PluginSkillsDocument{}, err
	}
	skill, err := ParseSkill(document, resolved.Slug)
	return protocol.PluginSkillsDocument{Skill: skill, Content: string(document)}, err
}

func (manager *Manager) InstallRegistry(ctx context.Context, source protocol.PluginSkillsRegistryResolved, name string) (protocol.PluginInfo, error) {
	if name != registryPackageName(source) {
		return protocol.PluginInfo{}, packageFault("registry skill identity does not match")
	}
	resolved, files, err := manager.acquireRegistry(ctx, protocol.PluginSkillsRegistrySource{Namespace: &source.Namespace, Slug: source.Slug, Version: &source.Version}, &source)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	document, err := normalizeRegistrySkill(files["SKILL.md"], resolved)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	skill, err := ParseSkill(document, source.Slug)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	original := files["SKILL.md"]
	files["SKILL.md"] = document
	entries, err := standaloneSkillEntries(files, skill, name, &source.Version, "https://skillhub.cn/skills/"+source.Namespace+"/"+source.Slug)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	entries = append(entries, packageEntry{name: "SOURCE-SKILL.md", data: original})
	sortPackageEntries(entries)
	info, err := manager.publish(ctx, entries, nil, name, protocol.PluginOrigin{Kind: "registry", Data: protocol.PluginOriginRegistryPayload{Source: resolved}})
	if err == nil {
		info.Skill = &protocol.PluginSkillsProvenance{Kind: "Registry", Data: protocol.PluginSkillsProvenanceRegistryPayload{Registry: resolved}}
	}
	return info, err
}
