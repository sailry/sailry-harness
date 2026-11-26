package protocol

import "github.com/sailry/sailry-harness/internal/protocol"

type (
	CommandCloseSshTerminalPayload           = protocol.CommandCloseSshTerminalPayload
	CommandInputTerminalPayload              = protocol.CommandInputTerminalPayload
	CommandInspectTerminalPayload            = protocol.CommandInspectTerminalPayload
	CommandListSshTerminalsPayload           = protocol.CommandListSshTerminalsPayload
	CommandResizeTerminalPayload             = protocol.CommandResizeTerminalPayload
	SshTerminalLaunch                        = protocol.SshTerminalLaunch
	TerminalAppearance                       = protocol.TerminalAppearance
	TerminalFrame                            = protocol.TerminalFrame
	TerminalInput                            = protocol.TerminalInput
	TerminalScreen                           = protocol.TerminalScreen
	TerminalScreenScreenUpdateReplacePayload = protocol.TerminalScreenScreenUpdateReplacePayload
	TerminalSnapshot                         = protocol.TerminalSnapshot
	TerminalViewport                         = protocol.TerminalViewport
)

const TerminalColorSchemeLight = protocol.TerminalColorSchemeLight
