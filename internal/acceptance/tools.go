package acceptance

import (
	"encoding/hex"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func (run *caseRun) capture(turn protocol.TurnID) (protocol.QueuedTurn, error) {
	if capture, ok := run.captures[turn]; ok {
		return capture, nil
	}
	output, err := run.command(run.ctx, "read_turn_capture", protocol.CommandReadTurnCapturePayload{Session: run.session.ID, Turn: turn})
	if err != nil {
		return protocol.QueuedTurn{}, err
	}
	capture, err := outputAs[protocol.QueuedTurn](output, "queued_turn")
	if err != nil {
		return capture, err
	}
	if capture.ID != turn || capture.Session != run.session.ID {
		return capture, fail("Turn capture belongs to another fixture")
	}
	if run.captures == nil {
		run.captures = map[protocol.TurnID]protocol.QueuedTurn{}
	}
	run.captures[turn] = capture
	return capture, nil
}

// PackageToolAlias returns the canonical identity defined by the package
// tool naming contract: the first eight BLAKE3 bytes of the owner name. Callers
// must verify the declared tool against the turn's captured package version;
// an arbitrary tool with a matching suffix is never accepted.
func PackageToolAlias(owner, name string) string {
	digest := blake3.Sum256([]byte(owner))
	return "plugin_" + hex.EncodeToString(digest[:8]) + "_" + name
}

func (run *caseRun) resolveToolNames(view turnView) (turnView, error) {
	calls := view.tools()
	if !slices.ContainsFunc(calls, func(call toolCall) bool { return strings.HasPrefix(call.name, "plugin_") }) {
		return view, nil
	}
	capture, err := run.capture(view.run.Turn)
	if err != nil {
		return view, err
	}
	view.aliases = map[string]string{}
	for _, reference := range capture.Plugins {
		// These cases exercise the files and commands packages. Other captured
		// packages cannot satisfy a fixture by declaring the same tool name.
		if reference.Name != "files" && reference.Name != "commands" {
			continue
		}
		prefix := PackageToolAlias(reference.Name, "")
		if !slices.ContainsFunc(calls, func(call toolCall) bool { return strings.HasPrefix(call.name, prefix) }) {
			continue
		}
		declared, known := run.pluginTools[reference]
		if !known {
			output, err := run.command(run.ctx, "read_plugin_version", protocol.CommandReadPluginVersionPayload{Package: reference})
			if err != nil {
				return view, err
			}
			info, err := outputAs[protocol.PluginInfo](output, "plugin")
			if err != nil {
				return view, err
			}
			if info.Summary.Name != reference.Name || info.Summary.Digest != reference.Digest || info.Extension == nil {
				return view, fail("Tool declaration differs from its captured package version")
			}
			declared = map[string]string{}
			for _, tool := range info.Extension.Tools {
				if tool.Server == nil {
					declared[PackageToolAlias(reference.Name, tool.Name)] = tool.Name
				}
			}
			if run.pluginTools == nil {
				run.pluginTools = map[protocol.PluginReference]map[string]string{}
			}
			run.pluginTools[reference] = declared
		}
		for alias, name := range declared {
			view.aliases[alias] = name
		}
	}
	return view, nil
}
