// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type PluginAction string

const (
	PluginActionReadArtifacts          PluginAction = "artifacts.read"
	PluginActionWriteArtifacts         PluginAction = "artifacts.write"
	PluginActionReadCodeIndex          PluginAction = "code.index.read"
	PluginActionReadCodeIndexSettings  PluginAction = "code.index.settings.read"
	PluginActionSaveCodeIndexSettings  PluginAction = "code.index.settings.write"
	PluginActionReadCode               PluginAction = "code.read"
	PluginActionManageCode             PluginAction = "code.manage"
	PluginActionReadCodeSettings       PluginAction = "code.settings.read"
	PluginActionSaveCodeSettings       PluginAction = "code.settings.write"
	PluginActionReadSearchSettings     PluginAction = "web.settings.read"
	PluginActionSaveSearchSettings     PluginAction = "web.settings.write"
	PluginActionReadActivity           PluginAction = "activity.read"
	PluginActionReadUsage              PluginAction = "usage.read"
	PluginActionReadDatabases          PluginAction = "databases.read"
	PluginActionControlDatabases       PluginAction = "databases.control"
	PluginActionManageDatabases        PluginAction = "databases.manage"
	PluginActionReadSsh                PluginAction = "ssh.read"
	PluginActionControlSsh             PluginAction = "ssh.control"
	PluginActionManageSsh              PluginAction = "ssh.manage"
	PluginActionDelegateAgents         PluginAction = "agents.delegate"
	PluginActionReadRoles              PluginAction = "roles.read"
	PluginActionReadTeams              PluginAction = "teams.read"
	PluginActionWriteRoles             PluginAction = "roles.write"
	PluginActionReadModels             PluginAction = "models.read"
	PluginActionRunDecisions           PluginAction = "decisions.run"
	PluginActionProvideModels          PluginAction = "models.provide"
	PluginActionAuthenticateModels     PluginAction = "models.authenticate"
	PluginActionInspectCredentials     PluginAction = "credentials.inspect"
	PluginActionInstallModels          PluginAction = "models.install"
	PluginActionInspectMedia           PluginAction = "media.inspect"
	PluginActionGenerateImage          PluginAction = "media.image"
	PluginActionGenerateVideo          PluginAction = "media.video"
	PluginActionReadMediaSettings      PluginAction = "media.settings.read"
	PluginActionSaveMediaSettings      PluginAction = "media.settings.write"
	PluginActionReadComputer           PluginAction = "computer.read"
	PluginActionControlComputer        PluginAction = "computer.control"
	PluginActionReadBrowser            PluginAction = "browser.read"
	PluginActionControlBrowser         PluginAction = "browser.control"
	PluginActionReadExternalBrowser    PluginAction = "external_browser.read"
	PluginActionControlExternalBrowser PluginAction = "external_browser.control"
	PluginActionNotify                 PluginAction = "notifications.publish"
	PluginActionDispatch               PluginAction = "dispatch.manage"
	PluginActionReadFiles              PluginAction = "files.read"
	PluginActionWriteFiles             PluginAction = "files.write"
	PluginActionReadGit                PluginAction = "git.read"
	PluginActionWriteGit               PluginAction = "git.write"
	PluginActionReadWorktrees          PluginAction = "worktrees.read"
	PluginActionWriteWorktrees         PluginAction = "worktrees.write"
	PluginActionReadCommands           PluginAction = "process.read"
	PluginActionControlCommands        PluginAction = "process.control"
	PluginActionReadTerminals          PluginAction = "terminals.read"
	PluginActionControlTerminals       PluginAction = "terminals.control"
	PluginActionReadConversation       PluginAction = "conversation.read"
	PluginActionControlConversation    PluginAction = "conversation.control"
	PluginActionReadMessage            PluginAction = "conversation.message.read"
	PluginActionStartSessions          PluginAction = "sessions.start"
	PluginActionReadProjects           PluginAction = "projects.read"
	PluginActionHttp                   PluginAction = "http.request"
	PluginActionReadWeb                PluginAction = "web.read"
	PluginActionGenerateText           PluginAction = "models.generate"
	PluginActionReadStorage            PluginAction = "storage.read"
	PluginActionWriteStorage           PluginAction = "storage.write"
	PluginActionReadKnowledge          PluginAction = "knowledge.read"
	PluginActionWriteKnowledge         PluginAction = "knowledge.write"
)

func (value PluginAction) MarshalJSON() ([]byte, error) {
	switch value {
	case PluginActionReadArtifacts, PluginActionWriteArtifacts, PluginActionReadCodeIndex, PluginActionReadCodeIndexSettings, PluginActionSaveCodeIndexSettings, PluginActionReadCode, PluginActionManageCode, PluginActionReadCodeSettings, PluginActionSaveCodeSettings, PluginActionReadSearchSettings, PluginActionSaveSearchSettings, PluginActionReadActivity, PluginActionReadUsage, PluginActionReadDatabases, PluginActionControlDatabases, PluginActionManageDatabases, PluginActionReadSsh, PluginActionControlSsh, PluginActionManageSsh, PluginActionDelegateAgents, PluginActionReadRoles, PluginActionReadTeams, PluginActionWriteRoles, PluginActionReadModels, PluginActionRunDecisions, PluginActionProvideModels, PluginActionAuthenticateModels, PluginActionInspectCredentials, PluginActionInstallModels, PluginActionInspectMedia, PluginActionGenerateImage, PluginActionGenerateVideo, PluginActionReadMediaSettings, PluginActionSaveMediaSettings, PluginActionReadComputer, PluginActionControlComputer, PluginActionReadBrowser, PluginActionControlBrowser, PluginActionReadExternalBrowser, PluginActionControlExternalBrowser, PluginActionNotify, PluginActionDispatch, PluginActionReadFiles, PluginActionWriteFiles, PluginActionReadGit, PluginActionWriteGit, PluginActionReadWorktrees, PluginActionWriteWorktrees, PluginActionReadCommands, PluginActionControlCommands, PluginActionReadTerminals, PluginActionControlTerminals, PluginActionReadConversation, PluginActionControlConversation, PluginActionReadMessage, PluginActionStartSessions, PluginActionReadProjects, PluginActionHttp, PluginActionReadWeb, PluginActionGenerateText, PluginActionReadStorage, PluginActionWriteStorage, PluginActionReadKnowledge, PluginActionWriteKnowledge:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown PluginAction %q", value)
	}
}
func (value *PluginAction) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginAction(decoded) {
	case PluginActionReadArtifacts, PluginActionWriteArtifacts, PluginActionReadCodeIndex, PluginActionReadCodeIndexSettings, PluginActionSaveCodeIndexSettings, PluginActionReadCode, PluginActionManageCode, PluginActionReadCodeSettings, PluginActionSaveCodeSettings, PluginActionReadSearchSettings, PluginActionSaveSearchSettings, PluginActionReadActivity, PluginActionReadUsage, PluginActionReadDatabases, PluginActionControlDatabases, PluginActionManageDatabases, PluginActionReadSsh, PluginActionControlSsh, PluginActionManageSsh, PluginActionDelegateAgents, PluginActionReadRoles, PluginActionReadTeams, PluginActionWriteRoles, PluginActionReadModels, PluginActionRunDecisions, PluginActionProvideModels, PluginActionAuthenticateModels, PluginActionInspectCredentials, PluginActionInstallModels, PluginActionInspectMedia, PluginActionGenerateImage, PluginActionGenerateVideo, PluginActionReadMediaSettings, PluginActionSaveMediaSettings, PluginActionReadComputer, PluginActionControlComputer, PluginActionReadBrowser, PluginActionControlBrowser, PluginActionReadExternalBrowser, PluginActionControlExternalBrowser, PluginActionNotify, PluginActionDispatch, PluginActionReadFiles, PluginActionWriteFiles, PluginActionReadGit, PluginActionWriteGit, PluginActionReadWorktrees, PluginActionWriteWorktrees, PluginActionReadCommands, PluginActionControlCommands, PluginActionReadTerminals, PluginActionControlTerminals, PluginActionReadConversation, PluginActionControlConversation, PluginActionReadMessage, PluginActionStartSessions, PluginActionReadProjects, PluginActionHttp, PluginActionReadWeb, PluginActionGenerateText, PluginActionReadStorage, PluginActionWriteStorage, PluginActionReadKnowledge, PluginActionWriteKnowledge:
		*value = PluginAction(decoded)
		return nil
	default:
		return fmt.Errorf("unknown PluginAction %q", decoded)
	}
}
func (value PluginAction) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginAction) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginActionAccess string

const (
	PluginActionAccessRead    PluginActionAccess = "read"
	PluginActionAccessModify  PluginActionAccess = "modify"
	PluginActionAccessOperate PluginActionAccess = "operate"
)

func (value PluginActionAccess) MarshalJSON() ([]byte, error) {
	switch value {
	case PluginActionAccessRead, PluginActionAccessModify, PluginActionAccessOperate:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown PluginActionAccess %q", value)
	}
}
func (value *PluginActionAccess) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginActionAccess(decoded) {
	case PluginActionAccessRead, PluginActionAccessModify, PluginActionAccessOperate:
		*value = PluginActionAccess(decoded)
		return nil
	default:
		return fmt.Errorf("unknown PluginActionAccess %q", decoded)
	}
}
func (value PluginActionAccess) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginActionAccess) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}
