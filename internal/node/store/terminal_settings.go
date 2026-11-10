package store

import (
	"context"
	"database/sql"
	"path/filepath"
	"slices"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func ReadTerminalSettings(ctx context.Context, db *sql.Conn) (protocol.TerminalSettings, error) {
	value, err := readBody[protocol.TerminalSettings](ctx, db, "SELECT body FROM terminal_settings WHERE singleton=1")
	if value == nil {
		return protocol.DefaultTerminalSettings(), err
	}
	return *value, err
}

// SaveTerminalSettings stores the default shell and environment of new
// terminals. The shell is resolved on this Node, so it must be absolute here.
func SaveTerminalSettings(ctx context.Context, db *sql.Conn, input protocol.TerminalSettings) (protocol.TerminalSettings, []protocol.Event, error) {
	if err := validateTerminalSettings(input); err != nil {
		return protocol.TerminalSettings{}, nil, err
	}
	if input.Shell != "" && !filepath.IsAbs(input.Shell) {
		return protocol.TerminalSettings{}, nil, invalid("terminal shell must be an absolute path on the execution Node")
	}
	current, err := ReadTerminalSettings(ctx, db)
	if err != nil {
		return protocol.TerminalSettings{}, nil, err
	}
	if current.Revision != input.Revision {
		return protocol.TerminalSettings{}, nil, fault(protocol.ErrorCodeRevisionConflict, "terminal settings changed")
	}
	if input.Revision, err = nextRevision(input.Revision); err != nil {
		return protocol.TerminalSettings{}, nil, err
	}
	body, err := encode(input)
	if err != nil {
		return protocol.TerminalSettings{}, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO terminal_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", body); err != nil {
		return protocol.TerminalSettings{}, nil, err
	}
	return input, []protocol.Event{{Kind: "terminal_settings_changed", Data: protocol.EventTerminalSettingsChangedPayload{Revision: input.Revision}}}, nil
}

var reservedTerminalVariables = []string{"TERM", "TERM_PROGRAM", "TERM_PROGRAM_VERSION", "COLORTERM", "COLORFGBG", "COLOR_SCHEME", "PWD", "VTE_VERSION", "BASH_ENV", "ENV", "ZDOTDIR", "IFS", "NODE_OPTIONS", "PERL5OPT", "PYTHONSTARTUP"}

func validateTerminalSettings(settings protocol.TerminalSettings) error {
	if len(settings.Shell) > 4096 || strings.ContainsFunc(settings.Shell, unicode.IsControl) {
		return invalid("invalid terminal shell path")
	}
	if settings.Shell == "" && len(settings.ShellArguments) != 0 {
		return invalid("terminal shell arguments require a shell path")
	}
	size, null := 0, false
	for _, argument := range settings.ShellArguments {
		size += len(argument)
		null = null || strings.ContainsRune(argument, 0)
	}
	if len(settings.ShellArguments) > 128 || size > 64*1024 || null {
		return invalid("invalid terminal shell arguments")
	}
	size = 0
	for key, value := range settings.Environment {
		upper := strings.ToUpper(key)
		reserved := strings.HasSuffix(upper, "_PRELOAD") || slices.Contains(reservedTerminalVariables, upper)
		for _, prefix := range []string{"SAILRY_", "DMUX_", "LD_", "DYLD_"} {
			reserved = reserved || strings.HasPrefix(upper, prefix)
		}
		if key == "" || key != strings.TrimSpace(key) || strings.Contains(key, "=") || strings.ContainsFunc(key, unicode.IsControl) || reserved {
			return invalid("invalid or reserved terminal environment name")
		}
		if strings.ContainsRune(value, 0) {
			return invalid("terminal environment value contains a null byte")
		}
		size += len(key) + len(value)
	}
	if size > 64*1024 {
		return invalid("terminal environment exceeds the supported size")
	}
	return nil
}

// ListSshTerminals returns the caller's open terminals of one SSH profile.
func ListSshTerminals(ctx context.Context, db *sql.Conn, profile protocol.SshID, caller protocol.NodeID) ([]protocol.TerminalInfo, error) {
	values, err := readBodies[protocol.TerminalInfo](ctx, db, "SELECT body FROM terminals ORDER BY id")
	if err != nil {
		return nil, err
	}
	return slices.DeleteFunc(values, func(info protocol.TerminalInfo) bool {
		return info.Ssh == nil || *info.Ssh != profile || info.Owner == nil || *info.Owner != caller || info.Status.Kind == "closed"
	}), nil
}

// ClosedTerminal resolves a close command for a terminal this Node process no
// longer runs: a stored terminal that matches the command and is not running
// becomes closed. It returns nil when the command's own fault stands.
func ClosedTerminal(ctx context.Context, db *sql.Conn, command protocol.Command) (*protocol.TerminalInfo, error) {
	var id protocol.TerminalID
	var worktree *protocol.WorktreeID
	switch input := command.Data.(type) {
	case *protocol.CommandCloseTerminalPayload:
		id, worktree = input.Terminal, &input.Worktree
	case *protocol.CommandCloseSshTerminalPayload:
		id = input.Terminal
	default:
		return nil, nil
	}
	info, err := readBody[protocol.TerminalInfo](ctx, db, "SELECT body FROM terminals WHERE id=?1", id.String())
	if err != nil || info == nil {
		return nil, err
	}
	matches := info.Ssh != nil
	if worktree != nil {
		matches = info.Worktree != nil && *info.Worktree == *worktree
	}
	if !matches || info.Status.Kind == "running" {
		return nil, nil
	}
	info.Status, info.Owner = protocol.TerminalStatus{Kind: "closed"}, nil
	info.Revision++
	return info, nil
}
