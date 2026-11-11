//go:build integration

package main

import (
	"errors"

	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

type fixtureConversationActions struct {
	failure error
}

func (actions *fixtureConversationActions) reject() {
	actions.failure = errors.New("conversation interaction requires the real Node acceptance fixture")
}

func (actions *fixtureConversationActions) OpenChild(string) { actions.reject() }
func (actions *fixtureConversationActions) Retry(string)     { actions.reject() }
func (actions *fixtureConversationActions) Fork(string)      { actions.reject() }
func (actions *fixtureConversationActions) Rewind(string)    { actions.reject() }
func (actions *fixtureConversationActions) Worktree(string)  { actions.reject() }
func (actions *fixtureConversationActions) Export(string, conversation.ExportFormat) {
	actions.reject()
}
func (actions *fixtureConversationActions) Read(string, string)        { actions.reject() }
func (actions *fixtureConversationActions) Edit(string)                { actions.reject() }
func (actions *fixtureConversationActions) OpenFile(conversation.File) { actions.reject() }
func (actions *fixtureConversationActions) FileAction(conversation.File, conversation.FileAction) {
	actions.reject()
}
func (actions *fixtureConversationActions) ImageAction(kit.ImageAction, conversation.Image) {
	actions.reject()
}
func (actions *fixtureConversationActions) Undo(string, string)   { actions.reject() }
func (actions *fixtureConversationActions) Review(string, string) { actions.reject() }
func (actions *fixtureConversationActions) LoadOlder()            { actions.reject() }
func (actions *fixtureConversationActions) Reconnect()            { actions.reject() }
func (actions *fixtureConversationActions) Quote(string)          { actions.reject() }
func (actions *fixtureConversationActions) SaveEdit(string, string) {
	actions.reject()
}
func (actions *fixtureConversationActions) CancelEdit(string) { actions.reject() }
func (actions *fixtureConversationActions) Start(string)      { actions.reject() }
