package core

import (
	"bytes"
	"encoding/hex"
	"encoding/json"
	"path/filepath"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func Worktrees(root string) string {
	return filepath.Join(root, ".sailry", "worktrees")
}

func SessionWorkspace(profile string, id protocol.SessionID) string {
	return filepath.Join(profile, "workspaces", "sessions", id.String())
}

func ConnectionWorkspace(profile string, resource protocol.ConnectionResource) (string, error) {
	var identity string
	switch resource.Kind {
	case "database":
		switch value := resource.Data.(type) {
		case protocol.DatabaseID:
			identity = value.String()
		case *protocol.DatabaseID:
			if value != nil {
				identity = value.String()
			}
		}
	case "ssh":
		switch value := resource.Data.(type) {
		case protocol.SshID:
			identity = value.String()
		case *protocol.SshID:
			if value != nil {
				identity = value.String()
			}
		}
	}
	if identity == "" {
		return "", &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "Node workspace identity is unavailable"}
	}
	return filepath.Join(profile, "connections", resource.Kind, identity, "workspace"), nil
}

func AssistantWorkspace(profile string, binding protocol.PluginConversationBinding) (string, error) {
	var identity bytes.Buffer
	encoder := json.NewEncoder(&identity)
	encoder.SetEscapeHTML(false)
	if err := encoder.Encode([2]string{binding.Package.Name, binding.ID}); err != nil {
		return "", &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "Node workspace identity is unavailable"}
	}
	digest := blake3.Sum256(bytes.TrimSuffix(identity.Bytes(), []byte{'\n'}))
	return filepath.Join(profile, "assistants", hex.EncodeToString(digest[:]), "workspace"), nil
}

// ManagedWorkspace accepts an exact registered root, never one of its descendants.
func ManagedWorkspace(root, profile string) bool {
	if !filepath.IsAbs(root) || !filepath.IsAbs(profile) {
		return false
	}
	relative, err := filepath.Rel(profile, root)
	if err != nil {
		return false
	}
	parts := strings.Split(relative, string(filepath.Separator))
	var expected string
	switch {
	case len(parts) == 2 && parts[0] == "worktrees":
		id, err := protocol.ParseWorktreeID(parts[1])
		if err != nil {
			return false
		}
		expected = filepath.Join(profile, "worktrees", id.String())
	case len(parts) == 3 && parts[0] == ".sailry" && parts[1] == "worktrees":
		id, err := protocol.ParseWorktreeID(parts[2])
		if err != nil {
			return false
		}
		expected = filepath.Join(Worktrees(profile), id.String())
	case len(parts) == 3 && parts[0] == "workspaces" && parts[1] == "sessions":
		id, err := protocol.ParseSessionID(parts[2])
		if err != nil {
			return false
		}
		expected = SessionWorkspace(profile, id)
	case len(parts) == 4 && parts[0] == "connections" && parts[3] == "workspace":
		switch parts[1] {
		case "database":
			id, err := protocol.ParseDatabaseID(parts[2])
			if err != nil {
				return false
			}
			expected, _ = ConnectionWorkspace(profile, protocol.ConnectionResource{Kind: "database", Data: id})
		case "ssh":
			id, err := protocol.ParseSshID(parts[2])
			if err != nil {
				return false
			}
			expected, _ = ConnectionWorkspace(profile, protocol.ConnectionResource{Kind: "ssh", Data: id})
		}
	case len(parts) == 3 && parts[0] == "assistants" && parts[2] == "workspace":
		digest, err := hex.DecodeString(parts[1])
		if err != nil || len(digest) != 32 {
			return false
		}
		expected = filepath.Join(profile, "assistants", hex.EncodeToString(digest), "workspace")
	}
	return expected != "" && expected == root
}
