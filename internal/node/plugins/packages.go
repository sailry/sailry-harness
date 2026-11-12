package plugins

import (
	"archive/zip"
	"bytes"
	"context"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"io"
	"io/fs"
	"net/http"
	"os"
	"path"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"unicode/utf8"

	sailry "github.com/sailry/sailry-harness"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

const MaxPackageEntries = 1024
const MaxPackageBytes = 128 * 1024 * 1024
const MaxPackageDepth = 32

type Manager struct {
	profile            string
	Client             *http.Client
	OfficialURL        string
	ThirdPartyURL      string
	RegistryURL        string
	SkillsDirectoryURL string
	repositoryMu       sync.Mutex
	repositories       map[string][]packageEntry
	skillPages         map[string]cachedSkillCatalog
}

type packageEntry struct {
	name       string
	directory  bool
	executable bool
	data       []byte
}

func NewManager(profile string) *Manager {
	if resolved, err := filepath.EvalSymlinks(profile); err == nil {
		profile = resolved
	}
	return &Manager{profile: profile, repositories: map[string][]packageEntry{}, skillPages: map[string]cachedSkillCatalog{}}
}

func (manager *Manager) source(directory, workingDirectory string) (string, error) {
	if workingDirectory != "" && !filepath.IsAbs(directory) && directory != "" && directory != "." && !resourcePath(filepath.ToSlash(directory)) {
		return "", packageFault("plugin source must be a normalized worktree-relative path")
	}
	if !filepath.IsAbs(directory) {
		directory = filepath.Join(workingDirectory, directory)
	}
	if metadata, err := os.Lstat(directory); err == nil && metadata.Mode()&fs.ModeSymlink != 0 {
		return "", packageFault("plugin sources must not be symbolic links")
	}
	directory, err := filepath.EvalSymlinks(directory)
	if err != nil {
		return "", err
	}
	if workingDirectory != "" {
		base, err := filepath.EvalSymlinks(workingDirectory)
		if err != nil {
			return "", err
		}
		base, err = filepath.Abs(base)
		if err != nil {
			return "", err
		}
		relative, err := filepath.Rel(base, directory)
		if err != nil || relative == ".." || strings.HasPrefix(relative, ".."+string(filepath.Separator)) {
			return "", packageFault("plugin source must remain inside its worktree")
		}
	}
	directory, err = filepath.Abs(directory)
	if err != nil {
		return "", err
	}
	profile, err := filepath.Abs(manager.profile)
	if err != nil {
		return "", err
	}
	if resolved, resolveErr := filepath.EvalSymlinks(profile); resolveErr == nil {
		profile = resolved
	}
	within := func(parent, child string) bool {
		relative, err := filepath.Rel(parent, child)
		return err == nil && relative != ".." && !strings.HasPrefix(relative, ".."+string(filepath.Separator))
	}
	if within(profile, directory) || within(directory, profile) {
		return "", packageFault("plugin source must not overlap the Node profile")
	}
	return directory, nil
}

type packageCaptureOptions struct {
	maximum int
	exclude func(string) bool
}

func capturePackage(ctx context.Context, tree fs.FS, open func(string) (io.ReadCloser, error), options ...packageCaptureOptions) ([]packageEntry, []protocol.PluginIssue, error) {
	rules := packageCaptureOptions{maximum: MaxPackageEntries}
	if len(options) > 0 {
		rules = options[0]
	}
	entries := []packageEntry{}
	issues := []protocol.PluginIssue{}
	total := int64(0)
	count := 0
	err := fs.WalkDir(tree, ".", func(name string, entry fs.DirEntry, walkErr error) error {
		if err := ctx.Err(); err != nil {
			return err
		}
		if walkErr != nil {
			return walkErr
		}
		if name == "." {
			return nil
		}
		if rules.exclude != nil && rules.exclude(name) {
			if entry.IsDir() {
				return fs.SkipDir
			}
			return nil
		}
		count++
		if count > rules.maximum || len(strings.Split(name, "/")) > MaxPackageDepth {
			return packageFault("plugin package exceeds its entry or depth limit")
		}
		metadata, err := entry.Info()
		if err != nil {
			return err
		}
		if !resourcePath(name) || (!metadata.IsDir() && !metadata.Mode().IsRegular()) {
			issues = append(issues, protocol.PluginIssue{Path: name, Kind: protocol.PluginIssueKindUnavailablePath})
			if entry.IsDir() {
				return fs.SkipDir
			}
			return nil
		}
		captured := packageEntry{name: name, directory: metadata.IsDir(), executable: metadata.Mode().Perm()&0o111 != 0}
		if captured.directory {
			entries = append(entries, captured)
			return nil
		}
		if metadata.Size() > MaxPackageBytes-total {
			return packageFault("plugin package exceeds its expanded size limit")
		}
		file, err := open(name)
		if err != nil {
			return err
		}
		if local, ok := file.(*os.File); ok {
			before, err := local.Stat()
			if err != nil || !os.SameFile(metadata, before) || metadata.Size() != before.Size() || metadata.Mode() != before.Mode() || !metadata.ModTime().Equal(before.ModTime()) {
				_ = file.Close()
				return &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "plugin source changed while being captured"}
			}
		}
		captured.data, err = io.ReadAll(io.LimitReader(file, MaxPackageBytes-total+1))
		if local, ok := file.(*os.File); ok && err == nil {
			after, inspectErr := local.Stat()
			if inspectErr != nil || int64(len(captured.data)) != metadata.Size() || after.Size() != metadata.Size() || metadata.Mode() != after.Mode() || !metadata.ModTime().Equal(after.ModTime()) {
				err = &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "plugin source changed while being captured"}
			}
		}
		closeErr := file.Close()
		if err != nil {
			return err
		}
		if closeErr != nil {
			return closeErr
		}
		total += int64(len(captured.data))
		if total > MaxPackageBytes {
			return packageFault("plugin package exceeds its expanded size limit")
		}
		entries = append(entries, captured)
		return nil
	})
	slices.SortFunc(entries, func(left, right packageEntry) int {
		return slices.Compare(strings.Split(left.name, "/"), strings.Split(right.name, "/"))
	})
	return entries, issues, err
}

func digestPackage(entries []packageEntry) string {
	var encoded bytes.Buffer
	var length [8]byte
	for _, entry := range entries {
		binary.LittleEndian.PutUint64(length[:], uint64(len(entry.name)))
		encoded.Write(length[:])
		encoded.WriteString(entry.name)
		if entry.directory {
			encoded.WriteString("directory")
			continue
		}
		encoded.WriteString("file")
		if entry.executable {
			encoded.WriteByte(1)
		} else {
			encoded.WriteByte(0)
		}
		binary.LittleEndian.PutUint64(length[:], uint64(len(entry.data)))
		encoded.Write(length[:])
		encoded.Write(entry.data)
	}
	var digest [32]byte
	blake3.DeriveKey(digest[:], "Sailry plugin package v1", encoded.Bytes())
	return hex.EncodeToString(digest[:])
}

func describePackage(entries []packageEntry, issues []protocol.PluginIssue, expected string) (protocol.PluginInfo, error) {
	files := map[string][]byte{}
	for _, entry := range entries {
		if !entry.directory {
			files[entry.name] = entry.data
		}
	}
	manifest, err := ValidateManifest(files["plugin.json"])
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	if expected != "" && manifest.Name != expected {
		return protocol.PluginInfo{}, packageFault("plugin package name differs from its selection")
	}
	info := protocol.DefaultPluginInfo()
	info.Summary.Name, info.Summary.Version, info.Summary.Description = manifest.Name, manifest.Version, manifest.Description
	info.Summary.Digest = digestPackage(entries)
	info.Extension = manifest.Extension
	info.Issues = append(manifest.Issues, issues...)
	info.Skills, issues = DiscoverSkills(files)
	info.Issues = append(info.Issues, issues...)
	var servers map[string]protocol.PluginMcpServer
	info.Mcp, servers, issues = ReadMCP(files["mcp.json"])
	info.Issues = append(info.Issues, issues...)
	if extension := info.Extension; extension != nil {
		info.Summary.Compatibility = Compatibility(extension.MinSailryVersion)
		if extension.Icon != nil {
			info.Icon = packageIcon(files[*extension.Icon])
			if info.Icon == nil {
				info.Issues = append(info.Issues, protocol.PluginIssue{Path: *extension.Icon, Kind: protocol.PluginIssueKindUnavailablePath})
			}
		}
		if extension.SettingsSchema != nil {
			settings, err := ValidateSettings(files[*extension.SettingsSchema], servers)
			if err != nil {
				info.Issues = append(info.Issues, protocol.PluginIssue{Path: *extension.SettingsSchema, Kind: protocol.PluginIssueKindInvalidExtensions})
				info.Extension = nil
			} else {
				info.Settings = &settings
			}
		}
		resources := []string{}
		if extension.Host != nil {
			resources = append(resources, extension.Host.Resources...)
		}
		if extension.Desktop != nil {
			resources = append(resources, extension.Desktop.Resources...)
		}
		for _, name := range resources {
			if _, exists := files[name]; !exists {
				info.Issues = append(info.Issues, protocol.PluginIssue{Path: name, Kind: protocol.PluginIssueKindUnavailablePath})
			}
		}
	}
	info.ReleaseNotes = releaseNotes(files, manifest.Version)
	return info, nil
}

func (manager *Manager) capture(ctx context.Context, directory, workingDirectory string, options ...packageCaptureOptions) ([]packageEntry, []protocol.PluginIssue, error) {
	source, err := manager.source(directory, workingDirectory)
	if err != nil {
		return nil, nil, err
	}
	root, err := os.OpenRoot(source)
	if err != nil {
		return nil, nil, err
	}
	defer root.Close()
	confined, err := core.OpenFiles(source, core.ManualFiles)
	if err != nil {
		return nil, nil, err
	}
	defer confined.Close()
	return capturePackage(ctx, root.FS(), func(name string) (io.ReadCloser, error) { return confined.Open(name) }, options...)
}

func (manager *Manager) Inspect(ctx context.Context, directory, workingDirectory, name string) (protocol.PluginInfo, error) {
	entries, issues, err := manager.capture(ctx, directory, workingDirectory)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	info, err := describePackage(entries, issues, name)
	info.Origin = &protocol.PluginOrigin{Kind: "directory"}
	return info, err
}

func (manager *Manager) Install(ctx context.Context, directory, workingDirectory, name string) (protocol.PluginInfo, error) {
	entries, issues, err := manager.capture(ctx, directory, workingDirectory)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	return manager.publish(ctx, entries, issues, name, protocol.PluginOrigin{Kind: "directory"})
}

func bundledCapture(ctx context.Context, name string) ([]packageEntry, []protocol.PluginIssue, error) {
	if !ValidateName(name) {
		return nil, nil, packageFault("invalid bundled package name")
	}
	tree, err := fs.Sub(sailry.Plugins, "plugins/"+name)
	if err != nil {
		return nil, nil, err
	}
	entries, issues, err := capturePackage(ctx, tree, func(name string) (io.ReadCloser, error) { return tree.Open(name) })
	for index := range entries {
		entries[index].executable = false
	}
	return entries, issues, err
}

func (manager *Manager) InspectBundled(ctx context.Context, name string) (protocol.PluginInfo, error) {
	entries, issues, err := bundledCapture(ctx, name)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	info, err := describePackage(entries, issues, name)
	info.Origin = &protocol.PluginOrigin{Kind: "bundled"}
	return info, err
}

func (manager *Manager) InstallBundled(ctx context.Context, name string) (protocol.PluginInfo, error) {
	entries, issues, err := bundledCapture(ctx, name)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	return manager.publish(ctx, entries, issues, name, protocol.PluginOrigin{Kind: "bundled"})
}

func (manager *Manager) publish(ctx context.Context, entries []packageEntry, issues []protocol.PluginIssue, name string, origin protocol.PluginOrigin) (protocol.PluginInfo, error) {
	info, err := describePackage(entries, issues, name)
	if err != nil {
		return info, err
	}
	info.Origin = &origin
	for _, issue := range info.Issues {
		if issue.Kind == protocol.PluginIssueKindInvalidSkills || issue.Kind == protocol.PluginIssueKindInvalidSkill {
			return info, packageFault("plugin skills must be valid before installation")
		}
	}
	if err := EnsureCompatible(info.Summary.Compatibility); err != nil {
		return info, err
	}
	packages := filepath.Join(manager.profile, "plugins", "packages")
	if err := os.MkdirAll(packages, 0o700); err != nil {
		return info, err
	}
	target := filepath.Join(packages, info.Summary.Digest)
	if _, err := os.Lstat(target); err == nil {
		if err := verifyPublished(ctx, target, info.Summary.Digest); err != nil {
			return info, err
		}
		return info, nil
	} else if !errors.Is(err, fs.ErrNotExist) {
		return info, err
	}
	staging, err := os.MkdirTemp(packages, ".staging-")
	if err != nil {
		return info, err
	}
	defer os.RemoveAll(staging)
	for _, entry := range entries {
		if err := ctx.Err(); err != nil {
			return info, err
		}
		filename := filepath.Join(staging, filepath.FromSlash(entry.name))
		if entry.directory {
			err = os.MkdirAll(filename, 0o700)
		} else {
			mode := fs.FileMode(0o600)
			if entry.executable {
				mode = 0o700
			}
			err = os.WriteFile(filename, entry.data, mode)
		}
		if err != nil {
			return info, err
		}
	}
	if err := os.Rename(staging, target); err != nil {
		if verifyErr := verifyPublished(ctx, target, info.Summary.Digest); verifyErr != nil {
			return info, err
		}
	}
	return info, nil
}

func verifyPublished(ctx context.Context, directory, expected string) error {
	metadata, err := os.Lstat(directory)
	if err != nil || !metadata.IsDir() || metadata.Mode()&fs.ModeSymlink != 0 {
		return packageFault("immutable plugin package is unavailable")
	}
	root, err := os.OpenRoot(directory)
	if err != nil {
		return err
	}
	defer root.Close()
	entries, issues, err := capturePackage(ctx, root.FS(), func(name string) (io.ReadCloser, error) { return root.Open(name) })
	if err != nil {
		return err
	}
	if len(issues) != 0 || digestPackage(entries) != expected {
		return packageFault("immutable plugin package content changed")
	}
	return nil
}

func (manager *Manager) PackageRoot(reference protocol.PluginReference) (string, error) {
	digest, err := hex.DecodeString(reference.Digest)
	if err != nil || len(digest) != 32 || hex.EncodeToString(digest) != reference.Digest || !ValidateName(reference.Name) {
		return "", packageFault("invalid plugin package reference")
	}
	directory := filepath.Join(manager.profile, "plugins", "packages", reference.Digest)
	metadata, err := os.Lstat(directory)
	if err != nil {
		return "", err
	}
	if !metadata.IsDir() || metadata.Mode()&fs.ModeSymlink != 0 {
		return "", packageFault("immutable plugin package is unavailable")
	}
	return directory, nil
}

// Collect removes the published package directories whose digest is not
// retained. Only canonical content-addressed directories belong to it; staging
// directories and foreign entries stay. An interrupted run leaves an orphan
// for the next one.
func (manager *Manager) Collect(retained map[string]struct{}) error {
	packages := filepath.Join(manager.profile, "plugins", "packages")
	for _, directory := range []string{filepath.Dir(packages), packages} {
		metadata, err := os.Lstat(directory)
		if errors.Is(err, fs.ErrNotExist) {
			return nil
		}
		if err != nil {
			return err
		}
		if !metadata.IsDir() {
			return packageFault("plugin package store is unavailable")
		}
	}
	entries, err := os.ReadDir(packages)
	if err != nil {
		return err
	}
	for _, entry := range entries {
		name := entry.Name()
		digest, err := hex.DecodeString(name)
		if _, held := retained[name]; held || err != nil || len(digest) != 32 || hex.EncodeToString(digest) != name || !entry.IsDir() {
			continue
		}
		if err := os.RemoveAll(filepath.Join(packages, name)); err != nil {
			return err
		}
	}
	return nil
}

func (manager *Manager) ReadResource(ctx context.Context, reference protocol.PluginReference, name string, limit int) ([]byte, error) {
	if !resourcePath(name) || limit < 0 || limit > MaxPackageBytes {
		return nil, packageFault("invalid plugin resource request")
	}
	entries, err := manager.captured(ctx, reference)
	if err != nil {
		return nil, err
	}
	for _, entry := range entries {
		if entry.name != name || entry.directory {
			continue
		}
		if len(entry.data) > limit {
			return nil, packageFault("plugin resource exceeds its size limit")
		}
		return entry.data, nil
	}
	return nil, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "plugin resource is missing"}
}

func (manager *Manager) InstallArchive(ctx context.Context, reader io.ReaderAt, size int64, name string) (protocol.PluginInfo, error) {
	return manager.archive(ctx, reader, size, name, true)
}

func (manager *Manager) archive(ctx context.Context, reader io.ReaderAt, size int64, name string, install bool) (protocol.PluginInfo, error) {
	archive, err := zip.NewReader(reader, size)
	if err != nil {
		return protocol.PluginInfo{}, packageFault("invalid plugin ZIP archive")
	}
	if len(archive.File) > MaxPackageEntries {
		return protocol.PluginInfo{}, packageFault("plugin archive exceeds its entry limit")
	}
	files := map[string]packageEntry{}
	total := uint64(0)
	for _, file := range archive.File {
		if err := ctx.Err(); err != nil {
			return protocol.PluginInfo{}, err
		}
		name := strings.TrimSuffix(file.Name, "/")
		if !resourcePath(name) || len(strings.Split(name, "/")) > MaxPackageDepth || file.Flags&1 != 0 ||
			(!file.Mode().IsRegular() && !file.FileInfo().IsDir()) {
			return protocol.PluginInfo{}, packageFault("plugin archive contains an unsupported entry")
		}
		if _, exists := files[name]; exists {
			return protocol.PluginInfo{}, packageFault("plugin archive contains duplicate entries")
		}
		entry := packageEntry{name: name, directory: file.FileInfo().IsDir(), executable: file.Mode().Perm()&0o111 != 0}
		if !entry.directory {
			if file.UncompressedSize64 > MaxPackageBytes-total {
				return protocol.PluginInfo{}, packageFault("plugin archive exceeds its expanded size limit")
			}
			source, err := file.Open()
			if err != nil {
				return protocol.PluginInfo{}, err
			}
			entry.data, err = io.ReadAll(io.LimitReader(source, int64(MaxPackageBytes-total)+1))
			closeErr := source.Close()
			if err != nil {
				return protocol.PluginInfo{}, err
			}
			if closeErr != nil {
				return protocol.PluginInfo{}, closeErr
			}
			total += uint64(len(entry.data))
			if total > MaxPackageBytes {
				return protocol.PluginInfo{}, packageFault("plugin archive exceeds its expanded size limit")
			}
		}
		files[name] = entry
	}
	prefix := ""
	if _, exists := files["plugin.json"]; !exists {
		for name := range files {
			if path.Base(name) == "plugin.json" && strings.Count(name, "/") == 1 {
				if prefix != "" {
					return protocol.PluginInfo{}, packageFault("plugin archive contains multiple packages")
				}
				prefix = path.Dir(name) + "/"
			}
		}
	}
	normalized := map[string]packageEntry{}
	for name, entry := range files {
		if prefix != "" {
			if name == strings.TrimSuffix(prefix, "/") && entry.directory {
				continue
			}
			if !strings.HasPrefix(name, prefix) {
				return protocol.PluginInfo{}, packageFault("plugin archive has content outside its package")
			}
			name = strings.TrimPrefix(name, prefix)
			entry.name = name
		}
		normalized[name] = entry
		for parent := path.Dir(name); parent != "."; parent = path.Dir(parent) {
			if existing, exists := normalized[parent]; exists && !existing.directory {
				return protocol.PluginInfo{}, packageFault("plugin archive has conflicting paths")
			}
			normalized[parent] = packageEntry{name: parent, directory: true}
		}
	}
	if len(normalized) > MaxPackageEntries {
		return protocol.PluginInfo{}, packageFault("plugin archive exceeds its entry limit")
	}
	entries := make([]packageEntry, 0, len(normalized))
	for _, entry := range normalized {
		entries = append(entries, entry)
	}
	slices.SortFunc(entries, func(left, right packageEntry) int {
		return slices.Compare(strings.Split(left.name, "/"), strings.Split(right.name, "/"))
	})
	if !install {
		info, err := describePackage(entries, nil, name)
		info.Origin = &protocol.PluginOrigin{Kind: "archive"}
		return info, err
	}
	return manager.publish(ctx, entries, nil, name, protocol.PluginOrigin{Kind: "archive"})
}

func releaseNotes(files map[string][]byte, version *string) *protocol.PluginCatalogReleaseNotes {
	if version == nil {
		return nil
	}
	section := func(data []byte) string {
		if len(data) > 256*1024 || !utf8.Valid(data) {
			return ""
		}
		lines := strings.Split(strings.ReplaceAll(string(data), "\r\n", "\n"), "\n")
		start := -1
		for index, line := range lines {
			if strings.HasPrefix(line, "## ") {
				if start >= 0 {
					return strings.TrimSpace(strings.Join(lines[start:index], "\n"))
				}
				if strings.TrimSpace(strings.TrimPrefix(line, "## ")) == *version {
					start = index + 1
				}
			}
		}
		if start >= 0 {
			return strings.TrimSpace(strings.Join(lines[start:], "\n"))
		}
		return ""
	}
	notes := protocol.PluginCatalogReleaseNotes{Text: section(files["CHANGELOG.md"]), Locales: map[string]string{}}
	for name, data := range files {
		if name != "CHANGELOG.md" && strings.HasPrefix(name, "CHANGELOG.") && strings.HasSuffix(name, ".md") {
			locale := strings.TrimSuffix(strings.TrimPrefix(name, "CHANGELOG."), ".md")
			if text := section(data); text != "" {
				notes.Locales[locale] = text
			}
		}
	}
	if notes.Text == "" {
		return nil
	}
	return &notes
}

func (manager *Manager) ReadBundle(ctx context.Context, reference protocol.PluginReference, entry string, resources []string) (map[string]string, error) {
	result := map[string]string{}
	for _, name := range append([]string{entry}, resources...) {
		if _, loaded := result[name]; loaded {
			continue
		}
		data, err := manager.ReadResource(ctx, reference, name, MaxPackageBytes)
		if err != nil {
			return nil, err
		}
		if !utf8.Valid(data) {
			return nil, packageFault("plugin script resources must be UTF-8")
		}
		result[name] = string(data)
	}
	return result, nil
}

func PackageReference(info protocol.PluginInfo) protocol.PluginReference {
	return protocol.PluginReference{Name: info.Summary.Name, Digest: info.Summary.Digest, SettingsRevision: info.Summary.SettingsRevision}
}
