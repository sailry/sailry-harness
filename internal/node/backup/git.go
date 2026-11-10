package backup

import (
	"bytes"
	"context"
	"crypto/sha256"
	"errors"
	"io"
	"io/fs"
	"maps"
	"os"
	"path/filepath"
	"strings"

	"github.com/go-git/go-billy/v5/osfs"
	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/cache"
	gitconfig "github.com/go-git/go-git/v5/plumbing/format/config"
	"github.com/go-git/go-git/v5/plumbing/format/packfile"
	"github.com/go-git/go-git/v5/plumbing/storer"
	"github.com/go-git/go-git/v5/storage/filesystem"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Entries of a linked checkout's metadata that stay meaningful without its owner.
var (
	commonMetadata = []string{"info/exclude", "info/attributes", "info/grafts", "info/sparse-checkout", "shallow"}
	localMetadata  = []string{
		"info/sparse-checkout", "ORIG_HEAD", "MERGE_HEAD", "MERGE_MSG", "MERGE_MODE", "CHERRY_PICK_HEAD",
		"REVERT_HEAD", "AUTO_MERGE", "SQUASH_MSG", "rebase-merge", "rebase-apply", "sequencer",
	}
)

type gitState struct {
	head          string
	references    map[string]string
	configuration []byte
	index         [sha256.Size]byte
	indexSize     int64
}

func (state gitState) equal(other gitState) bool {
	return state.head == other.head && maps.Equal(state.references, other.references) &&
		bytes.Equal(state.configuration, other.configuration) && state.index == other.index && state.indexSize == other.indexSize
}

// prepareGit builds independent Git metadata for a linked checkout without
// writing to the source. It returns the replacement `.git` directory, or an
// empty path when the checkout owns its metadata.
func prepareGit(ctx context.Context, source, temporary string) (string, error) {
	if err := cancelled(ctx); err != nil {
		return "", err
	}
	info, err := os.Lstat(filepath.Join(source, ".git"))
	if errors.Is(err, fs.ErrNotExist) || err == nil && info.IsDir() {
		return "", nil
	}
	if err != nil {
		return "", ioFault(err)
	}
	if !info.Mode().IsRegular() {
		return "", invalid("unsupported backup Git metadata entry")
	}
	local, common, err := gitDirectories(source)
	if err != nil || common == "" {
		return "", err
	}
	repository, err := gogit.PlainOpenWithOptions(source, &gogit.PlainOpenOptions{EnableDotGitCommonDir: true})
	if err != nil {
		return "", ioFault(err)
	}
	before, err := readGitState(ctx, repository, local, common)
	if err != nil {
		return "", err
	}
	directory, err := os.MkdirTemp(temporary, "backup-git-")
	if err != nil {
		return "", ioFault(err)
	}
	if err := copyGit(ctx, repository, before, local, common, directory); err != nil {
		_ = os.RemoveAll(directory)
		return "", err
	}
	after, err := readGitState(ctx, repository, local, common)
	if err == nil && !after.equal(before) {
		err = gitChanged()
	}
	if err != nil {
		_ = os.RemoveAll(directory)
		return "", err
	}
	return directory, nil
}

// gitDirectories resolves a pointer file to its private and shared metadata.
// The shared directory is empty unless the checkout is a linked worktree.
func gitDirectories(source string) (local, common string, err error) {
	pointer, err := os.ReadFile(filepath.Join(source, ".git"))
	if err != nil {
		return "", "", ioFault(err)
	}
	value, ok := strings.CutPrefix(strings.TrimSpace(string(pointer)), "gitdir:")
	if !ok {
		return "", "", invalid("unsupported backup Git metadata entry")
	}
	local = strings.TrimSpace(value)
	if !filepath.IsAbs(local) {
		local = filepath.Join(source, local)
	}
	shared, err := os.ReadFile(filepath.Join(local, "commondir"))
	if errors.Is(err, fs.ErrNotExist) {
		return local, "", nil
	}
	if err != nil {
		return "", "", ioFault(err)
	}
	common = strings.TrimSpace(string(shared))
	if !filepath.IsAbs(common) {
		common = filepath.Join(local, common)
	}
	return local, common, nil
}

func readGitState(ctx context.Context, repository *gogit.Repository, local, common string) (gitState, error) {
	head, err := repository.Storer.Reference(plumbing.HEAD)
	if err != nil {
		return gitState{}, ioFault(err)
	}
	state := gitState{head: head.String(), references: map[string]string{}}
	references, err := repository.Storer.IterReferences()
	if err != nil {
		return gitState{}, ioFault(err)
	}
	defer references.Close()
	size := 0
	err = references.ForEach(func(reference *plumbing.Reference) error {
		if err := cancelled(ctx); err != nil {
			return err
		}
		if reference.Name() == plumbing.HEAD {
			return nil
		}
		value := reference.String()
		if size += len(value); size > MaxManifest {
			return invalid("backup Git references exceed the limit")
		}
		state.references[reference.Name().String()] = value
		return nil
	})
	if err != nil {
		return gitState{}, gitFault(err)
	}
	for _, path := range []string{filepath.Join(common, "config"), filepath.Join(local, "config.worktree")} {
		body, err := os.ReadFile(path)
		if errors.Is(err, fs.ErrNotExist) {
			continue
		}
		if err != nil {
			return gitState{}, ioFault(err)
		}
		if len(state.configuration)+len(body) > MaxManifest {
			return gitState{}, invalid("backup Git configuration exceeds the limit")
		}
		state.configuration = append(append(state.configuration, body...), '\n')
	}
	index, err := os.Open(filepath.Join(local, "index"))
	if errors.Is(err, fs.ErrNotExist) {
		state.indexSize = -1
		return state, nil
	}
	if err != nil {
		return gitState{}, ioFault(err)
	}
	defer index.Close()
	sum := sha256.New()
	if state.indexSize, err = io.Copy(sum, index); err != nil {
		return gitState{}, ioFault(err)
	}
	copy(state.index[:], sum.Sum(nil))
	return state, nil
}

func copyGit(ctx context.Context, repository *gogit.Repository, state gitState, local, common, directory string) error {
	destination := filesystem.NewStorage(osfs.New(directory), cache.NewObjectLRUDefault())
	if _, err := gogit.Init(destination, nil); err != nil {
		return ioFault(err)
	}
	if err := copyGitObjects(ctx, repository, destination); err != nil {
		return err
	}
	references, err := repository.Storer.IterReferences()
	if err != nil {
		return ioFault(err)
	}
	err = references.ForEach(func(reference *plumbing.Reference) error {
		if reference.Name() == plumbing.HEAD {
			return nil
		}
		return destination.SetReference(reference)
	})
	references.Close()
	if err != nil {
		return ioFault(err)
	}
	head, err := repository.Storer.Reference(plumbing.HEAD)
	if err != nil {
		return ioFault(err)
	}
	if err := destination.SetReference(head); err != nil {
		return ioFault(err)
	}
	if state.indexSize >= 0 {
		body, err := os.ReadFile(filepath.Join(local, "index"))
		if err != nil {
			return ioFault(err)
		}
		if int64(len(body)) != state.indexSize || sha256.Sum256(body) != state.index {
			return gitChanged()
		}
		if err := os.WriteFile(filepath.Join(directory, "index"), body, 0o600); err != nil {
			return ioFault(err)
		}
	}
	if err := copyGitMetadata(ctx, local, common, directory); err != nil {
		return err
	}
	return writeGitConfiguration(state.configuration, directory)
}

func copyGitObjects(ctx context.Context, repository *gogit.Repository, destination *filesystem.Storage) error {
	objects, err := repository.Storer.IterEncodedObjects(plumbing.AnyObject)
	if err != nil {
		return ioFault(err)
	}
	var hashes []plumbing.Hash
	var total uint64
	err = objects.ForEach(func(object plumbing.EncodedObject) error {
		if err := cancelled(ctx); err != nil {
			return err
		}
		if total += uint64(object.Size()); total > MaxBytes {
			return invalid("backup Git objects exceed the limit")
		}
		hashes = append(hashes, object.Hash())
		return nil
	})
	objects.Close()
	if err != nil {
		return gitFault(err)
	}
	if len(hashes) == 0 {
		return nil
	}
	// One pack keeps the archive's file count independent of the history size.
	writer, err := destination.ObjectStorage.PackfileWriter()
	if err != nil {
		return ioFault(err)
	}
	_, err = packfile.NewEncoder(writer, storer.EncodedObjectStorer(repository.Storer), false).Encode(hashes, 0)
	if err := errors.Join(err, writer.Close()); err != nil {
		return ioFault(err)
	}
	return cancelled(ctx)
}

func copyGitMetadata(ctx context.Context, local, common, directory string) error {
	localRoot, err := os.OpenRoot(local)
	if err != nil {
		return ioFault(err)
	}
	defer localRoot.Close()
	commonRoot, err := os.OpenRoot(common)
	if err != nil {
		return ioFault(err)
	}
	defer commonRoot.Close()
	output, err := os.OpenRoot(directory)
	if err != nil {
		return ioFault(err)
	}
	defer output.Close()
	budget := &gitBudget{}
	for _, name := range commonMetadata {
		if err := budget.copy(ctx, commonRoot, output, name); err != nil {
			return err
		}
	}
	names, err := entries(localRoot)
	if err != nil {
		return err
	}
	// A split index retains its adjacent shared index, without retaining gitdir links.
	for _, name := range names {
		if strings.HasPrefix(name, "sharedindex.") {
			if err := budget.copy(ctx, localRoot, output, name); err != nil {
				return err
			}
		}
	}
	for _, name := range localMetadata {
		if err := budget.copy(ctx, localRoot, output, name); err != nil {
			return err
		}
	}
	return nil
}

type gitBudget struct {
	size  uint64
	count int
}

func (budget *gitBudget) copy(ctx context.Context, source, output *os.Root, relative string) error {
	if err := cancelled(ctx); err != nil {
		return err
	}
	parts, err := components(relative)
	if err != nil {
		return err
	}
	if len(parts) > 64 {
		return invalid("backup Git metadata nesting exceeds the limit")
	}
	parent, err := descend(source, parts[:len(parts)-1])
	if err != nil {
		// A missing or non-directory parent means the optional entry is absent.
		return nil
	}
	defer parent.Close()
	name := parts[len(parts)-1]
	info, err := parent.Lstat(name)
	if errors.Is(err, fs.ErrNotExist) {
		return nil
	}
	if err != nil {
		return ioFault(err)
	}
	if info.IsDir() {
		directory, err := parent.OpenRoot(name)
		if err != nil {
			return ioFault(err)
		}
		defer directory.Close()
		names, err := entries(directory)
		if err != nil {
			return err
		}
		for _, name := range names {
			if err := budget.copy(ctx, source, output, relative+"/"+name); err != nil {
				return err
			}
		}
		return nil
	}
	input, before, err := openRegular(parent, name)
	if err != nil {
		return err
	}
	defer input.Close()
	budget.size += uint64(before.Size())
	budget.count++
	if budget.size > MaxBytes || budget.count > MaxFiles {
		return invalid("backup Git metadata exceeds the limit")
	}
	file, err := output.OpenFile(relative, os.O_WRONLY|os.O_CREATE|os.O_TRUNC, 0o600)
	if errors.Is(err, fs.ErrNotExist) {
		if err = output.MkdirAll(strings.Join(parts[:len(parts)-1], "/"), 0o700); err == nil {
			file, err = output.OpenFile(relative, os.O_WRONLY|os.O_CREATE|os.O_TRUNC, 0o600)
		}
	}
	if err != nil {
		return ioFault(err)
	}
	_, _, err = copyHash(ctx, input, file, uint64(before.Size()))
	if closeErr := file.Close(); err == nil {
		err = wrapIO(closeErr)
	}
	if err != nil {
		if code(err) == protocol.ErrorCodeCancelled {
			return err
		}
		return gitChanged()
	}
	after, err := input.Stat()
	if err != nil {
		return ioFault(err)
	}
	if before.Size() != after.Size() || !before.ModTime().Equal(after.ModTime()) {
		return gitChanged()
	}
	return nil
}

// writeGitConfiguration keeps the checkout's own settings and drops the
// directives that would retain an external owner.
func writeGitConfiguration(source []byte, directory string) error {
	configuration := gitconfig.New()
	if err := gitconfig.NewDecoder(bytes.NewReader(source)).Decode(configuration); err != nil {
		return ioFault(err)
	}
	configuration.RemoveSection("include").RemoveSection("includeIf").RemoveSection("includeif")
	configuration.Section("core").RemoveOption("worktree").RemoveOption("bare")
	if configuration.HasSection("extensions") {
		configuration.Section("extensions").RemoveOption("worktreeconfig").RemoveOption("worktreeConfig")
	}
	configuration.SetOption("core", gitconfig.NoSubsection, "bare", "false")
	configuration.SetOption("core", gitconfig.NoSubsection, "worktree", "..")
	var output bytes.Buffer
	if err := gitconfig.NewEncoder(&output).Encode(configuration); err != nil {
		return ioFault(err)
	}
	return wrapIO(os.WriteFile(filepath.Join(directory, "config"), output.Bytes(), 0o600))
}

func gitFault(err error) error {
	var failure *protocol.Fault
	if errors.As(err, &failure) {
		return failure
	}
	return ioFault(err)
}

func gitChanged() error {
	return fault(protocol.ErrorCodeRevisionConflict, "Git metadata changed while creating the backup")
}
