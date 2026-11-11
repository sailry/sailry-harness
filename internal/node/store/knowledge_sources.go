package store

import (
	"context"
	"database/sql"
	"errors"
	"io/fs"
	"math"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// knowledgePackage resolves the plugin a request acts as. Admission already
// checked the running turn or callback.
func knowledgePackage(ctx context.Context, db *sql.Conn, scope protocol.PluginContext) (protocol.PluginInfo, error) {
	if scope.Turn != nil || scope.Invocation != nil {
		return ResolvePluginInfo(ctx, db, scope.Package)
	}
	info, err := currentKnowledgePlugin(ctx, db, scope.Package.Name)
	if err != nil {
		return info, err
	}
	if (protocol.PluginReference{Name: info.Summary.Name, Digest: info.Summary.Digest, SettingsRevision: info.Summary.SettingsRevision}) != scope.Package {
		return info, fault(protocol.ErrorCodeRevisionConflict, "plugin package changed")
	}
	return info, nil
}

func currentKnowledgePlugin(ctx context.Context, db *sql.Conn, name string) (protocol.PluginInfo, error) {
	info, err := ReadPlugin(ctx, db, name)
	if err != nil {
		return protocol.PluginInfo{}, err
	}
	if info == nil {
		return protocol.PluginInfo{}, fault(protocol.ErrorCodeNotFound, "knowledge owner plugin is unavailable")
	}
	if !info.Summary.Enabled || info.Summary.Compatibility.Kind != "compatible" {
		return protocol.PluginInfo{}, fault(protocol.ErrorCodeNotConfigured, "knowledge owner plugin is disabled")
	}
	return *info, nil
}

func knowledgeOwnership(ctx context.Context, db *sql.Conn, request protocol.Request, selected *string) (protocol.KnowledgeOwner, KnowledgeCapture, error) {
	plugin := func(info protocol.PluginInfo) (protocol.KnowledgeOwner, *protocol.PluginReference) {
		return protocol.KnowledgeOwner{Kind: "plugin", Data: protocol.KnowledgeOwnerPluginPayload{Name: info.Summary.Name}},
			&protocol.PluginReference{Name: info.Summary.Name, Digest: info.Summary.Digest, SettingsRevision: info.Summary.SettingsRevision}
	}
	if scope := request.Plugin; scope != nil {
		if selected != nil {
			return protocol.KnowledgeOwner{}, KnowledgeCapture{}, knowledgeDenied("plugin collection ownership comes from its captured package")
		}
		info, err := knowledgePackage(ctx, db, *scope)
		if err != nil {
			return protocol.KnowledgeOwner{}, KnowledgeCapture{}, err
		}
		owner, reference := plugin(info)
		return owner, KnowledgeCapture{Package: reference, Worktree: scope.Worktree, Session: scope.Session}, nil
	}
	if selected != nil {
		info, err := currentKnowledgePlugin(ctx, db, *selected)
		if err != nil {
			return protocol.KnowledgeOwner{}, KnowledgeCapture{}, err
		}
		owner, reference := plugin(info)
		return owner, KnowledgeCapture{Package: reference}, nil
	}
	return protocol.KnowledgeOwner{Kind: "controller"}, KnowledgeCapture{}, nil
}

func knowledgeDeclares(info protocol.PluginInfo, action protocol.PluginAction) bool {
	return info.Extension != nil && slices.Contains(info.Extension.Actions, action)
}

func knowledgeReadable(stored KnowledgeStored, info protocol.PluginInfo) bool {
	source := ReadKnowledgeSource(stored.Collection.Definition.Source)
	switch source.Kind {
	case "worktree":
		return knowledgeDeclares(info, protocol.PluginActionReadFiles)
	case "storage":
		return knowledgeDeclares(info, protocol.PluginActionReadStorage) &&
			(source.Scope != protocol.PluginStorageScopeConversation || knowledgeDeclares(info, protocol.PluginActionReadConversation))
	}
	return true
}

// knowledgeRelative reports a normalized worktree-relative source path; the
// root is "" or ".".
func knowledgeRelative(path string) bool {
	return path == "" || path == "." || fs.ValidPath(path) && !strings.ContainsAny(path, "\\:") && !strings.ContainsFunc(path, unicode.IsControl)
}

func knowledgeIO(err error) error {
	switch {
	case errors.Is(err, fs.ErrNotExist):
		return fault(protocol.ErrorCodeNotFound, "knowledge source does not exist")
	case errors.Is(err, fs.ErrPermission):
		return fault(protocol.ErrorCodePermissionDenied, "knowledge source cannot be read")
	}
	return fault(protocol.ErrorCodeUnavailable, "knowledge source is unavailable: "+err.Error())
}

// knowledgeTarget requires a regular file or directory below root whose
// parents are ordinary directories.
func knowledgeTarget(root string, parts []string) error {
	directory, err := os.OpenRoot(root)
	if err != nil {
		return knowledgeIO(err)
	}
	defer directory.Close()
	for index := range parts {
		metadata, err := directory.Lstat(filepath.Join(parts[:index+1]...))
		if err != nil {
			return knowledgeIO(err)
		}
		last := index == len(parts)-1
		if metadata.Mode()&os.ModeSymlink != 0 || !metadata.IsDir() && !(last && metadata.Mode().IsRegular()) {
			if last {
				return invalid("knowledge sources require regular files or directories")
			}
			return invalid("file parents must be ordinary directories")
		}
	}
	return nil
}

func validateKnowledge(ctx context.Context, db *sql.Conn, request protocol.Request, definition protocol.KnowledgeDefinition, capture *KnowledgeCapture) error {
	if err := knowledge.ValidateDefinition(definition); err != nil {
		return err
	}
	for _, pattern := range definition.Globs {
		if _, err := knowledge.Glob(pattern); err != nil {
			return invalid("invalid knowledge source glob")
		}
	}
	var info *protocol.PluginInfo
	if request.Plugin != nil {
		captured, err := knowledgePackage(ctx, db, *request.Plugin)
		if err != nil {
			return err
		}
		info = &captured
	}
	source := ReadKnowledgeSource(definition.Source)
	switch source.Kind {
	case "worktree":
		if scope := request.Plugin; scope != nil && (scope.Worktree == nil || *scope.Worktree != source.Worktree || !knowledgeDeclares(*info, protocol.PluginActionReadFiles)) {
			return knowledgeDenied("knowledge source is outside the captured file access")
		}
		var root string
		err := db.QueryRowContext(ctx, "SELECT path FROM worktrees WHERE id=?1", source.Worktree.String()).Scan(&root)
		if errors.Is(err, sql.ErrNoRows) {
			return fault(protocol.ErrorCodeNotFound, "knowledge worktree is unavailable")
		}
		if err != nil {
			return err
		}
		if !knowledgeRelative(source.Path) {
			return invalid("expected a normalized worktree-relative path")
		}
		var parts []string
		if source.Path != "" && source.Path != "." {
			parts = strings.Split(source.Path, "/")
		}
		if err := knowledgeTarget(root, parts); err != nil {
			return err
		}
		worktree := source.Worktree
		capture.Worktree, capture.Session = &worktree, nil
	case "directory", "file":
		if request.Plugin != nil {
			return knowledgeDenied("absolute knowledge sources require a controller selection")
		}
		native, err := knowledge.Native(source.Path)
		if err != nil {
			return err
		}
		if slices.ContainsFunc(strings.Split(filepath.ToSlash(native), "/"), func(part string) bool { return part == ".." || part == "." }) {
			return invalid("knowledge sources require an absolute path")
		}
		metadata, err := os.Lstat(native)
		if err != nil {
			return knowledgeIO(err)
		}
		if source.Kind == "directory" && !metadata.IsDir() || source.Kind == "file" && !metadata.Mode().IsRegular() {
			return invalid("knowledge sources require regular files or directories")
		}
		capture.Worktree, capture.Session = nil, nil
	case "storage":
		if capture.Package == nil {
			return knowledgeDenied("knowledge storage sources require a real plugin owner")
		}
		if info != nil && (!knowledgeDeclares(*info, protocol.PluginActionReadStorage) ||
			source.Scope == protocol.PluginStorageScopeConversation && !knowledgeDeclares(*info, protocol.PluginActionReadConversation)) {
			return knowledgeDenied("plugin does not declare the captured storage access")
		}
		if source.Scope != protocol.PluginStorageScopeConversation {
			capture.Worktree, capture.Session = nil, nil
			break
		}
		if capture.Session == nil {
			return knowledgeDenied("conversation storage requires a captured session")
		}
		var worktree string
		err := db.QueryRowContext(ctx, "SELECT worktree FROM sessions WHERE id=?1 AND state!='removed'", capture.Session.String()).Scan(&worktree)
		if errors.Is(err, sql.ErrNoRows) {
			return fault(protocol.ErrorCodeNotFound, "knowledge conversation is unavailable")
		}
		if err != nil {
			return err
		}
		if capture.Worktree == nil || capture.Worktree.String() != worktree {
			return knowledgeDenied("conversation storage is outside the captured worktree")
		}
	}
	for _, name := range definition.SharedWith {
		if _, err := currentKnowledgePlugin(ctx, db, name); err != nil {
			return err
		}
	}
	return nil
}

func checkKnowledge(stored KnowledgeStored) error {
	collection := stored.Collection
	if err := knowledge.ValidateDefinition(collection.Definition); err != nil {
		return err
	}
	for _, pattern := range collection.Definition.Globs {
		if _, err := knowledge.Glob(pattern); err != nil {
			return invalid("invalid stored knowledge source glob")
		}
	}
	if collection.Revision == 0 || collection.Revision > math.MaxInt64 || collection.Rebuild > math.MaxInt64 ||
		collection.Progress.Generation == 0 || collection.Progress.Generation > math.MaxInt64 || collection.Progress.Completed > collection.Progress.Sources {
		return invalid("invalid stored knowledge collection")
	}
	if stored.Automatic && !knowledgeAutomatic(stored) {
		return invalid("invalid automatic knowledge collection")
	}
	source := ReadKnowledgeSource(collection.Definition.Source)
	switch {
	case collection.Owner.Kind == "controller" && stored.Capture.Package == nil && source.Kind != "storage":
	case collection.Owner.Kind == "plugin" && stored.Capture.Package != nil && knowledgeOwner(collection.Owner) == stored.Capture.Package.Name:
		if !plugins.ValidateName(stored.Capture.Package.Name) {
			return invalid("invalid plugin name")
		}
		if stored.Capture.Package.SettingsRevision > math.MaxInt64 {
			return invalid("invalid captured plugin configuration revision")
		}
	default:
		return invalid("knowledge owner does not match its source capture")
	}
	captured := stored.Capture.Session != nil || stored.Capture.Worktree != nil
	switch {
	case source.Kind == "worktree":
		if !knowledgeRelative(source.Path) {
			return invalid("expected a normalized worktree-relative path")
		}
		if stored.Capture.Worktree == nil || *stored.Capture.Worktree != source.Worktree || stored.Capture.Session != nil {
			return invalid("invalid knowledge worktree capture")
		}
	case source.Kind == "storage" && source.Scope == protocol.PluginStorageScopeConversation && stored.Capture.Session != nil && stored.Capture.Worktree != nil:
	case source.Kind == "storage" && source.Scope == protocol.PluginStorageScopeNode && !captured:
	case (source.Kind == "directory" || source.Kind == "file") && !captured:
		if !filepath.IsAbs(source.Path) || slices.Contains(strings.Split(filepath.ToSlash(source.Path), "/"), "..") {
			return invalid("invalid stored absolute knowledge source")
		}
	default:
		return invalid("invalid knowledge source capture")
	}
	return nil
}
