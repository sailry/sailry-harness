package plugins

import (
	"archive/tar"
	"archive/zip"
	"bytes"
	"compress/gzip"
	"context"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path"
	"path/filepath"
	"slices"
	"strings"

	sailry "github.com/sailry/sailry-harness"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func SkillImportDigest(data []byte) string { return fmt.Sprintf("%x", blake3.Sum256(data)) }

func importedSkillName(directory, skill string) string {
	digest := blake3.Sum256([]byte(directory + "\x00" + skill))
	return fmt.Sprintf("skill-import-%x", digest[:12])
}

func readSkillArchive(ctx context.Context, data []byte, format protocol.PluginSkillsArchiveFormat, metadata ...*skillEntryMetadata) (map[string][]byte, error) {
	if len(data) == 0 || len(data) > MaxPackageBytes {
		return nil, packageFault("skill archive exceeds its size limit")
	}
	files, names := map[string][]byte{}, map[string]bool{}
	count, total := 0, int64(0)
	appendEntry := func(name string, directory bool, mode fs.FileMode, size int64, reader io.Reader) error {
		if err := ctx.Err(); err != nil {
			return err
		}
		name = strings.TrimSuffix(name, "/")
		count++
		if !resourcePath(name) || names[name] || count > 10000 || len(strings.Split(name, "/")) > 32 || size < 0 || size > MaxPackageBytes-total {
			return packageFault("invalid skill archive entry")
		}
		names[name] = true
		if directory {
			if len(metadata) > 0 {
				metadata[0].directories = append(metadata[0].directories, name)
			}
			return nil
		}
		body, err := io.ReadAll(io.LimitReader(reader, size+1))
		if err != nil {
			return err
		}
		if int64(len(body)) != size {
			return packageFault("skill archive entry size changed")
		}
		total += int64(len(body))
		files[name] = body
		if len(metadata) > 0 {
			metadata[0].executable[name] = mode.Perm()&0o111 != 0
		}
		return nil
	}
	switch format {
	case protocol.PluginSkillsArchiveFormatZip:
		archive, err := zip.NewReader(bytes.NewReader(data), int64(len(data)))
		if err != nil {
			return nil, packageFault("invalid skill ZIP archive")
		}
		for _, file := range archive.File {
			if file.Flags&1 != 0 || !file.FileInfo().IsDir() && !file.Mode().IsRegular() || file.UncompressedSize64 > MaxPackageBytes {
				return nil, packageFault("skill archive contains unsupported files")
			}
			reader, err := file.Open()
			if err != nil {
				return nil, err
			}
			err = appendEntry(file.Name, file.FileInfo().IsDir(), file.Mode(), int64(file.UncompressedSize64), reader)
			closeErr := reader.Close()
			if err != nil {
				return nil, err
			}
			if closeErr != nil {
				return nil, closeErr
			}
		}
	case protocol.PluginSkillsArchiveFormatTar, protocol.PluginSkillsArchiveFormatTarGz:
		var reader io.Reader = bytes.NewReader(data)
		if format == protocol.PluginSkillsArchiveFormatTarGz {
			compressed, err := gzip.NewReader(reader)
			if err != nil {
				return nil, packageFault("invalid compressed skill archive")
			}
			defer compressed.Close()
			reader = compressed
		}
		archive := tar.NewReader(reader)
		for {
			header, err := archive.Next()
			if err == io.EOF {
				break
			}
			if err != nil {
				return nil, packageFault("invalid skill TAR archive")
			}
			if header.Typeflag != tar.TypeReg && header.Typeflag != tar.TypeRegA && header.Typeflag != tar.TypeDir {
				return nil, packageFault("skill archive contains unsupported files")
			}
			if err := appendEntry(header.Name, header.Typeflag == tar.TypeDir, fs.FileMode(header.Mode), header.Size, archive); err != nil {
				return nil, err
			}
		}
	default:
		return nil, packageFault("unsupported skill archive format")
	}
	for name := range names {
		for parent := path.Dir(name); parent != "."; parent = path.Dir(parent) {
			if _, exists := files[parent]; exists {
				return nil, packageFault("skill archive contains conflicting paths")
			}
		}
	}
	return files, nil
}

func discoverImportedSkills(files map[string][]byte) ([]protocol.PluginSkillsCandidate, error) {
	paths := []string{}
	for filename := range files {
		if path.Base(filename) == "SKILL.md" {
			paths = append(paths, filename)
		}
	}
	slices.SortFunc(paths, func(left, right string) int {
		return slices.Compare(strings.Split(left, "/"), strings.Split(right, "/"))
	})
	result := []protocol.PluginSkillsCandidate{}
	selected := []string{}
	for _, filename := range paths {
		directory := path.Dir(filename)
		if directory == "." {
			directory = ""
		}
		skip := false
		for _, parent := range selected {
			skip = skip || parent == "" || strings.HasPrefix(directory, parent+"/")
		}
		for _, part := range strings.Split(directory, "/") {
			skip = skip || slices.Contains([]string{".git", "node_modules", "target", ".venv"}, part)
		}
		if skip {
			continue
		}
		skill, err := ParseSkill(files[filename], "")
		if err != nil {
			return nil, err
		}
		result = append(result, protocol.PluginSkillsCandidate{Name: importedSkillName(directory, skill.Name), Path: directory, Skill: skill})
		selected = append(selected, directory)
		if len(result) > MaxSkills {
			return nil, packageFault("skill import contains too many skills")
		}
	}
	if len(result) == 0 {
		return nil, packageFault("skill import contains no SKILL.md")
	}
	return result, nil
}

func (manager *Manager) DiscoverSkillImport(ctx context.Context, data []byte, stream protocol.StreamID, format protocol.PluginSkillsArchiveFormat, kind protocol.PluginUploadSource) (protocol.PluginSkillsImportDiscovery, error) {
	if kind != protocol.PluginUploadSourceDirectory && kind != protocol.PluginUploadSourceArchive {
		return protocol.PluginSkillsImportDiscovery{}, packageFault("invalid skill import source")
	}
	files, err := readSkillArchive(ctx, data, format)
	if err != nil {
		return protocol.PluginSkillsImportDiscovery{}, err
	}
	skills, err := discoverImportedSkills(files)
	return protocol.PluginSkillsImportDiscovery{Stream: stream, Digest: SkillImportDigest(data), Kind: kind, Skills: skills}, err
}

func (manager *Manager) DiscoverSkillUpload(ctx context.Context, reader io.ReaderAt, size int64, stream protocol.StreamID, format protocol.PluginSkillsArchiveFormat, kind protocol.PluginUploadSource) (protocol.PluginSkillsImportDiscovery, error) {
	if size <= 0 || size > MaxPackageBytes {
		return protocol.PluginSkillsImportDiscovery{}, packageFault("skill archive exceeds its size limit")
	}
	data, err := io.ReadAll(io.NewSectionReader(reader, 0, size))
	if err != nil {
		return protocol.PluginSkillsImportDiscovery{}, err
	}
	if int64(len(data)) != size {
		return protocol.PluginSkillsImportDiscovery{}, packageFault("skill upload size changed")
	}
	return manager.DiscoverSkillImport(ctx, data, stream, format, kind)
}

func (manager *Manager) InstallSkillUpload(ctx context.Context, reader io.ReaderAt, size int64, format protocol.PluginSkillsArchiveFormat, source protocol.PluginSkillsImported, name string) (protocol.PluginInfo, error) {
	if size <= 0 || size > MaxPackageBytes {
		return protocol.PluginInfo{}, packageFault("skill archive exceeds its size limit")
	}
	data, err := io.ReadAll(io.NewSectionReader(reader, 0, size))
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	if int64(len(data)) != size {
		return protocol.PluginInfo{}, packageFault("skill upload size changed")
	}
	return manager.InstallSkillImport(ctx, data, format, source, name)
}

func (manager *Manager) InstallSkillImport(ctx context.Context, data []byte, format protocol.PluginSkillsArchiveFormat, source protocol.PluginSkillsImported, name string) (protocol.PluginInfo, error) {
	if source.Digest != SkillImportDigest(data) || source.Path != "" && !resourcePath(source.Path) ||
		source.Kind != protocol.PluginUploadSourceDirectory && source.Kind != protocol.PluginUploadSourceArchive {
		return protocol.PluginInfo{}, &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "skill import changed; discover it again"}
	}
	metadata := skillEntryMetadata{executable: map[string]bool{}}
	files, err := readSkillArchive(ctx, data, format, &metadata)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	selected := map[string][]byte{}
	selectedMetadata := skillEntryMetadata{executable: map[string]bool{}}
	for filename, data := range files {
		if source.Path == "" {
			selected[filename] = data
			selectedMetadata.executable[filename] = metadata.executable[filename]
		} else if strings.HasPrefix(filename, source.Path+"/") {
			name := strings.TrimPrefix(filename, source.Path+"/")
			selected[name] = data
			selectedMetadata.executable[name] = metadata.executable[filename]
		}
	}
	for _, directory := range metadata.directories {
		if source.Path == "" {
			selectedMetadata.directories = append(selectedMetadata.directories, directory)
		} else if strings.HasPrefix(directory, source.Path+"/") {
			selectedMetadata.directories = append(selectedMetadata.directories, strings.TrimPrefix(directory, source.Path+"/"))
		}
	}
	skill, err := ParseSkill(selected["SKILL.md"], "")
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	if importedSkillName(source.Path, skill.Name) != name {
		return protocol.PluginInfo{}, packageFault("skill identity does not match the imported directory")
	}
	entries, err := standaloneSkillEntries(selected, skill, name, nil, "", selectedMetadata)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	for _, filename := range sourceLicenseNames {
		if data, exists := files[filename]; exists {
			if len(data) > 1024*1024 {
				return protocol.PluginInfo{}, packageFault("source license exceeds its size limit")
			}
			entries = append(entries, packageEntry{name: "SOURCE-" + filename, data: data})
		}
	}
	if err := validateSkillPackage(entries); err != nil {
		return protocol.PluginInfo{}, err
	}
	sortPackageEntries(entries)
	info, err := manager.publish(ctx, entries, nil, name, protocol.PluginOrigin{Kind: "archive"})
	if err == nil {
		info.Skill = &protocol.PluginSkillsProvenance{Kind: "Imported", Data: protocol.PluginSkillsProvenanceImportedPayload{Imported: source}}
	}
	return info, err
}

func (manager *Manager) SnapshotSkills(ctx context.Context, directory, workingDirectory string) ([]byte, protocol.PluginSkillsArchiveFormat, protocol.PluginUploadSource, error) {
	source, err := manager.source(directory, workingDirectory)
	if err != nil {
		return nil, "", "", err
	}
	metadata, err := os.Lstat(source)
	if err != nil {
		return nil, "", "", err
	}
	if !metadata.IsDir() && filepath.Base(source) == "SKILL.md" {
		source = filepath.Dir(source)
		metadata, err = os.Stat(source)
		if err != nil {
			return nil, "", "", err
		}
	}
	if !metadata.IsDir() {
		format := protocol.PluginSkillsArchiveFormatZip
		lower := strings.ToLower(source)
		switch {
		case strings.HasSuffix(lower, ".tar.gz") || strings.HasSuffix(lower, ".tgz"):
			format = protocol.PluginSkillsArchiveFormatTarGz
		case strings.HasSuffix(lower, ".tar"):
			format = protocol.PluginSkillsArchiveFormatTar
		case strings.HasSuffix(lower, ".zip") || strings.HasSuffix(lower, ".skill"):
		default:
			return nil, "", "", packageFault("select a skill directory or archive")
		}
		confined, err := core.OpenFiles(filepath.Dir(source), core.ManualFiles)
		if err != nil {
			return nil, "", "", err
		}
		defer confined.Close()
		file, err := confined.Open(filepath.Base(source))
		if err != nil {
			return nil, "", "", err
		}
		defer file.Close()
		before, err := file.Stat()
		if err != nil {
			return nil, "", "", err
		}
		data, err := io.ReadAll(io.LimitReader(file, MaxPackageBytes+1))
		if err != nil || len(data) > MaxPackageBytes {
			return nil, "", "", packageFault("skill archive exceeds its size limit")
		}
		after, err := file.Stat()
		if err != nil {
			return nil, "", "", err
		}
		if int64(len(data)) != before.Size() || before.Size() != after.Size() || !before.ModTime().Equal(after.ModTime()) {
			return nil, "", "", &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "skill archive changed while being captured"}
		}
		return data, format, protocol.PluginUploadSourceArchive, nil
	}
	entries, issues, err := manager.capture(ctx, source, "", packageCaptureOptions{maximum: 10000, exclude: excludedSkillSnapshotPath})
	if err != nil {
		return nil, "", "", err
	}
	if len(issues) != 0 {
		return nil, "", "", packageFault("skill directory contains unsupported files")
	}
	var output bytes.Buffer
	archive := zip.NewWriter(&output)
	for _, entry := range entries {
		header := &zip.FileHeader{Name: entry.name, Method: zip.Deflate}
		if entry.directory {
			header.Name += "/"
			header.SetMode(fs.ModeDir | 0o700)
		} else if entry.executable {
			header.SetMode(0o700)
		} else {
			header.SetMode(0o600)
		}
		writer, err := archive.CreateHeader(header)
		if err != nil {
			return nil, "", "", err
		}
		if !entry.directory {
			if _, err := writer.Write(entry.data); err != nil {
				return nil, "", "", err
			}
		}
	}
	if err := archive.Close(); err != nil {
		return nil, "", "", err
	}
	return output.Bytes(), protocol.PluginSkillsArchiveFormatZip, protocol.PluginUploadSourceDirectory, nil
}

func excludedSkillSnapshotPath(name string) bool {
	parts := strings.Split(name, "/")
	for index, part := range parts {
		if strings.EqualFold(part, ".git") {
			return true
		}
		if index > 0 && strings.EqualFold(parts[index-1], ".sailry") && strings.EqualFold(part, "worktrees") {
			return true
		}
	}
	return false
}

func (manager *Manager) ReadSkillDocument(ctx context.Context, source protocol.PluginSkillsDocumentSource) (protocol.PluginSkillsDocument, error) {
	var data []byte
	var err error
	switch source.Kind {
	case "registry":
		var input protocol.PluginSkillsRegistryResolved
		if err := decodePackageData(source.Data, &input); err != nil {
			return protocol.PluginSkillsDocument{}, err
		}
		return manager.RegistryDocument(ctx, input)
	case "bundled":
		var input protocol.PluginSkillsDocumentSourceBundledPayload
		if err := decodePackageData(source.Data, &input); err != nil {
			return protocol.PluginSkillsDocument{}, err
		}
		if !ValidateName(input.Package) || !skillName(input.Skill) {
			return protocol.PluginSkillsDocument{}, packageFault("invalid bundled skill identity")
		}
		data, err = fs.ReadFile(sailry.Plugins, "plugins/"+input.Package+"/skills/"+input.Skill+"/SKILL.md")
	case "installed":
		var input protocol.PluginSkillsDocumentSourceInstalledPayload
		if err := decodePackageData(source.Data, &input); err != nil {
			return protocol.PluginSkillsDocument{}, err
		}
		if !skillName(input.Skill) {
			return protocol.PluginSkillsDocument{}, packageFault("invalid installed skill identity")
		}
		data, err = manager.ReadResource(ctx, input.Package, "skills/"+input.Skill+"/SKILL.md", MaxSkillDocumentBytes)
	case "git":
		var input protocol.PluginSkillsDocumentSourceGitPayload
		if err := decodePackageData(source.Data, &input); err != nil {
			return protocol.PluginSkillsDocument{}, err
		}
		selected, parseErr := ParseRepositorySource(protocol.PluginSkillsSource{Repository: input.Source.Repository, GitRef: &input.Source.GitRef, Path: &input.Path})
		if parseErr != nil || selected.Repository != input.Source.Repository || !commitHash(input.Source.Commit) {
			return protocol.PluginSkillsDocument{}, packageFault("invalid captured skill source")
		}
		_, entries, issues, readErr := acquireRepository(ctx, selected, input.Source.Commit)
		if readErr != nil {
			return protocol.PluginSkillsDocument{}, readErr
		}
		if len(issues) != 0 {
			return protocol.PluginSkillsDocument{}, packageFault("skill source contains unsupported files")
		}
		for _, entry := range entries {
			if entry.name == "SKILL.md" {
				data = entry.data
			}
		}
	default:
		return protocol.PluginSkillsDocument{}, packageFault("unsupported skill document source")
	}
	if err != nil {
		return protocol.PluginSkillsDocument{}, err
	}
	skill, err := ParseSkill(data, "")
	return protocol.PluginSkillsDocument{Skill: skill, Content: string(data)}, err
}

func (manager *Manager) InspectArchive(ctx context.Context, reader io.ReaderAt, size int64, name string) (protocol.PluginInfo, error) {
	return manager.archive(ctx, reader, size, name, false)
}
