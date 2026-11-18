package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type controllerLoginFixture struct {
	storage               *Store
	node, controller      protocol.NodeID
	ctx                   context.Context
	provider              protocol.ConversationProvider
	session               protocol.SessionID
	turn                  protocol.TurnID
	previous, replacement protocol.CredentialRef
	old, next             protocol.RuntimeCredential
	waiting               protocol.ConversationRunWaiting
	manifestRevision      uint64
}

func newControllerLoginFixture(t *testing.T) *controllerLoginFixture {
	t.Helper()
	storage, node, controller, ctx := controllerCacheFixture(t)
	f := &controllerLoginFixture{storage: storage, node: node, controller: controller, ctx: ctx}
	f.provider = providerFixture()
	f.provider.Revision, f.provider.Authentication = 1, protocol.AuthenticationClaudeOAuth
	f.provider.Api, f.provider.Endpoint = protocol.ConversationModelApiAnthropic, "https://api.anthropic.com"
	f.previous = protocol.CredentialRef{Node: controller, ID: protocol.NewCredentialID()}
	f.replacement = protocol.CredentialRef{Node: controller, ID: protocol.NewCredentialID()}
	f.provider.Credential = &f.previous
	expired, future := uint64(1), uint64(time.Now().Add(time.Hour).UnixMilli())
	f.old = protocol.RuntimeCredential{Controller: controller, Kind: "provider", ID: f.previous.ID.String(), Generation: f.previous.ID.String(), Revision: 1,
		State: "ready", Provider: &f.provider.ID, Authentication: f.provider.Authentication, Endpoint: f.provider.Endpoint,
		Access: protocol.NewSecret("expired-fixture-access"), ExpiresAtMs: &expired, AccountIdentity: "trusted-account-workspace", AuthenticationTarget: "captured-auth-target"}
	f.next = f.old
	f.next.ID, f.next.Generation, f.next.Access, f.next.ExpiresAtMs = f.replacement.ID.String(), f.replacement.ID.String(), protocol.NewSecret("fresh-fixture-access"), &future
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if err := f.publish(t, db, false); err != nil {
			return err
		}
		session, turn, err := createAdmittedLocalProviderTurn(ctx, db, storage, node, controller, f.provider.ID, time.Now())
		if err != nil {
			return err
		}
		if err := installHistoricalRemoteCapture(ctx, db, session, turn.ID, controller, f.provider); err != nil {
			return err
		}
		f.session = session.ID
		invocation, _, err := ClaimNext(ctx, db, time.Now())
		if err != nil {
			return err
		}
		if invocation == nil {
			return fmt.Errorf("fixture task was not claimed")
		}
		f.turn = turn.ID
		if _, err := SetRunCredentialWait(ctx, db, node, f.turn, &protocol.ConversationRunWaiting{Kind: "controller_login", Credential: f.previous, Provider: f.provider.ID}); err != nil {
			return err
		}
		run, err := ReadAgentRun(ctx, db, f.turn)
		if err == nil {
			f.waiting = *run.Waiting
		}
		return err
	})
	if err != nil {
		t.Fatal(err)
	}
	return f
}

func (f *controllerLoginFixture) publish(t *testing.T, db *sql.Conn, replacement bool) error {
	t.Helper()
	f.manifestRevision++
	records := []protocol.ConfigurationFollowRecord{followRecord(t, f.old.CacheID(), f.old.Revision, f.old, nil)}
	if replacement {
		records = append(records, followRecord(t, f.next.CacheID(), f.next.Revision, f.next, nil))
	}
	_, err := applyConfigurationCache(f.ctx, db, f.node, f.controller, protocol.ConfigurationManifest{Source: f.controller, Category: "runtime_credentials", Revision: f.manifestRevision, Records: records})
	return err
}

func (f *controllerLoginFixture) request() protocol.Request {
	return protocol.NewRequest(f.node, protocol.Command{Kind: "resume_controller_login", Data: &protocol.CommandResumeControllerLoginPayload{
		Session: f.session, Turn: f.turn, Waiting: f.waiting.Attempt, Previous: f.previous, Replacement: f.replacement,
	}})
}

func controllerLoginReason(err error) string {
	var fault *protocol.Fault
	if errors.As(err, &fault) && fault.Details != nil && fault.Details.Kind == "controller_login" {
		if detail, ok := fault.Details.Data.(*protocol.FaultDetailsControllerLoginPayload); ok {
			return detail.Reason
		}
	}
	return ""
}

func TestControllerLoginRecoveryKeepsAdmissionAndOnlyChangesEffectiveCredential(t *testing.T) {
	t.Parallel()
	f := newControllerLoginFixture(t)
	err := f.storage.Node.Write(f.ctx, func(db *sql.Conn) error {
		capture, err := readTurnCapture(f.ctx, db, f.turn)
		if err != nil {
			return err
		}
		before, err := json.Marshal(capture)
		if err != nil {
			return err
		}
		input := *f.request().Command.Data.(*protocol.CommandResumeControllerLoginPayload)
		if available, err := ControllerLoginReplacementAvailable(f.ctx, db, f.node, input); err != nil || available {
			return fmt.Errorf("new login was available before configuration push: %v", err)
		}
		if err := f.publish(t, db, true); err != nil {
			return err
		}
		request := f.request()
		output, _, err := ExecuteAgentCommand(f.ctx, db, f.node, f.storage.Profile, f.controller, request, time.Now())
		if err != nil {
			return err
		}
		if run := output.Data.(protocol.ConversationRun); run.Waiting != nil || run.Status != protocol.ConversationStatusRunning {
			return fmt.Errorf("recovery changed task status or left it waiting")
		}
		effective, err := EffectiveTurnCredential(f.ctx, db, f.turn, f.previous)
		if err != nil || effective != f.replacement {
			return fmt.Errorf("effective credential was not persisted: %v", err)
		}
		capture, err = readTurnCapture(f.ctx, db, f.turn)
		if err != nil {
			return err
		}
		after, err := json.Marshal(capture)
		if err != nil || string(after) != string(before) {
			return fmt.Errorf("recovery changed original admission capture: %v", err)
		}
		if _, _, err := ExecuteAgentCommand(f.ctx, db, f.node, f.storage.Profile, f.controller, f.request(), time.Now()); controllerLoginReason(err) != "state_changed" {
			return fmt.Errorf("a second request restarted the old wait: %v", err)
		}
		var count int
		if err := db.QueryRowContext(f.ctx, "SELECT count(*) FROM agent_credential_recoveries WHERE turn=?1", f.turn.String()).Scan(&count); err != nil {
			return err
		}
		if count != 1 {
			return fmt.Errorf("recorded %d recovery effects", count)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestNodeOwnedProviderLoginRestoresPairedUsersTask(t *testing.T) {
	t.Parallel()
	storage, node, controller, ctx := controllerCacheFixture(t)
	now := time.Now()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		info := pluginTestInfo("local-account-fixture", 'a')
		info.Summary.Enabled, info.Summary.Revision = true, 1
		extension := protocol.DefaultPluginExtension()
		extension.ApiVersion = "v1"
		extension.Actions = []protocol.PluginAction{protocol.PluginActionAuthenticateModels, protocol.PluginActionInspectCredentials}
		authentication, client := protocol.AuthenticationChatGpt, "oaiapp_fixture"
		declaration := protocol.DefaultPluginModelAuthDeclaration()
		declaration.ID, declaration.Authentication, declaration.Client.ClientID = "account", &authentication, &client
		declaration.Flow = protocol.PluginModelAuthFlow{Kind: "script", Data: protocol.PluginModelAuthFlowScriptPayload{
			Script: protocol.PluginModelAdapterScript{Entry: "auth.js", Resources: []string{}}, Refresh: true, Handler: "step", Requests: map[string]protocol.PluginModelAuthEndpoint{},
		}}
		extension.ModelAuth, info.Extension = []protocol.PluginModelAuthDeclaration{declaration}, &extension
		if _, err := InstallPlugin(ctx, db, info, 0, true); err != nil {
			return err
		}
		provider := providerFixture()
		provider.Revision, provider.Authentication = 1, authentication
		provider.Api, provider.Endpoint = protocol.ConversationModelApiResponses, "https://api.openai.com/v1"
		if err := saveProviderConfiguration(ctx, db, providerConfiguration{ID: provider.ID, Listed: true, Provider: &provider}); err != nil {
			return err
		}
		grant := providers.AccountGrant{
			Kind: authentication, Access: "initial-access", Refresh: "initial-refresh", IDToken: "initial-identity",
			ClientID: client, Subject: "local-account", ExpiresAtMs: uint64(now.Add(time.Hour).UnixMilli()),
			Scopes: []string{"resource.invoke", "chatgpt.tokens.use.direct"},
		}
		signIn := func(revision uint64, grant providers.AccountGrant) (*providerConfiguration, error) {
			login, err := CaptureProviderLogin(ctx, db, provider.ID, revision, protocol.NewRequestID())
			if err != nil {
				return nil, err
			}
			if _, err := CompleteProviderLogin(ctx, db, node, login, grant); err != nil {
				return nil, err
			}
			return currentProvider(ctx, db, provider.ID, nil)
		}
		saved, err := signIn(provider.Revision, grant)
		if err != nil {
			return err
		}
		previous := *saved.Provider.Credential
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model, config.Credential = saved.Provider.ID, saved.Provider.DefaultModel, &previous
		config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
		created, _, err := createSession(ctx, db, node, storage.Profile, protocol.CommandCreateSessionPayload{Config: &config})
		if err != nil {
			return err
		}
		session := created.Data.(protocol.Session)
		turn, err := admitCredentialClaim(ctx, db, storage, node, controller, session, now)
		if err != nil {
			return err
		}
		expired := grant
		expired.ExpiresAtMs = 1
		body, err := json.Marshal(expired)
		if err != nil {
			return err
		}
		entry := saved.Authorizations[previous.ID]
		entry.Key, entry.ExpiresAtMs = pointerSecret(string(body)), expired.Expiry()
		saved.Authorizations[previous.ID] = entry
		if err := saveProviderConfiguration(ctx, db, *saved); err != nil {
			return err
		}
		invocation, _, err := ClaimNext(ctx, db, now)
		if err != nil || invocation == nil || invocation.Turn.ID != turn.ID {
			return fmt.Errorf("claim the paired user's task: %v", err)
		}
		if _, err := ReadProviderAuthorization(ctx, db, node, *invocation.Provider, previous); !providerFixtureFault(err, protocol.ErrorCodeExpired) {
			return fmt.Errorf("expired Node-owned authorization did not permit login recovery: %v", err)
		}
		if _, err := SetRunCredentialWait(ctx, db, node, turn.ID, &protocol.ConversationRunWaiting{
			Kind: "controller_login", Credential: previous, Provider: provider.ID,
		}); err != nil {
			return err
		}
		run, err := ReadAgentRun(ctx, db, turn.ID)
		if err != nil || run.Waiting == nil || run.Waiting.AccountIdentity == "" || run.Waiting.AuthenticationTarget == "" {
			return fmt.Errorf("task did not record its Node-owned account identity: %v", err)
		}
		grant.Access = "replacement-access"
		replacement, err := signIn(saved.Provider.Revision, grant)
		if err != nil {
			return err
		}
		input := protocol.CommandResumeControllerLoginPayload{
			Session: session.ID, Turn: turn.ID, Waiting: run.Waiting.Attempt,
			Previous: previous, Replacement: *replacement.Provider.Credential,
		}
		if available, err := ControllerLoginReplacementAvailable(ctx, db, node, input); err != nil || !available {
			return fmt.Errorf("Node-owned replacement waited for an unrelated configuration push: %v", err)
		}
		wrongNode := input
		wrongNode.Replacement.Node = controller
		if _, err := CheckControllerLoginWait(ctx, db, node, controller, wrongNode); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
			return fmt.Errorf("login recovery allowed a replacement from a different Node: %v", err)
		}
		request := protocol.NewRequest(node, protocol.Command{Kind: "resume_controller_login", Data: &input})
		if _, _, err := ExecuteAgentCommand(ctx, db, node, storage.Profile, controller, request, now); err != nil {
			return err
		}
		effective, err := EffectiveTurnCredential(ctx, db, turn.ID, previous)
		if err != nil || effective != input.Replacement {
			return fmt.Errorf("login recovery did not retain the target Node's account: %v", err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestControllerLoginRecoveryRejectsOtherAccountTargetAndChangedTask(t *testing.T) {
	t.Parallel()
	for _, name := range []string{"account", "workspace", "missing identity", "target", "provider", "controller", "stopped", "completed", "new wait"} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			f := newControllerLoginFixture(t)
			err := f.storage.Node.Write(f.ctx, func(db *sql.Conn) error {
				expected := "account_mismatch"
				request := f.request()
				switch name {
				case "account", "workspace":
					f.next.AccountIdentity = "different-" + name
				case "missing identity":
					f.next.AccountIdentity, expected = "", "identity_unavailable"
				case "target":
					f.next.AuthenticationTarget, expected = "different-target", "target_changed"
				case "provider":
					other := protocol.NewProviderID()
					f.next.Provider, expected = &other, "target_changed"
				case "controller":
					request.Command.Data.(*protocol.CommandResumeControllerLoginPayload).Replacement.Node = protocol.NodeID{91}
					expected = "target_changed"
				case "stopped", "completed", "new wait":
					run, err := ReadAgentRun(f.ctx, db, f.turn)
					if err != nil {
						return err
					}
					if name == "new wait" {
						run.Waiting.Attempt = protocol.NewRequestID()
					} else if name == "stopped" {
						run.Status = protocol.ConversationStatusCancelled
					} else {
						run.Status = protocol.ConversationStatusCompleted
					}
					if err := putAgentRun(f.ctx, db, run); err != nil {
						return err
					}
					expected = "state_changed"
				}
				if err := f.publish(t, db, true); err != nil {
					return err
				}
				_, _, err := ExecuteAgentCommand(f.ctx, db, f.node, f.storage.Profile, f.controller, request, time.Now())
				if controllerLoginReason(err) != expected {
					return fmt.Errorf("%s recovery returned %v, expected %s", name, err, expected)
				}
				effective, err := EffectiveTurnCredential(f.ctx, db, f.turn, f.previous)
				if err != nil || effective != f.previous {
					return fmt.Errorf("rejected recovery replaced the task account: %v", err)
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestControllerLoginWaitEnrichesLegacyIdentityWithoutChangingAttempt(t *testing.T) {
	t.Parallel()
	for _, name := range []string{"both missing", "account missing", "target missing", "account changed", "target changed"} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			f := newControllerLoginFixture(t)
			err := f.storage.Node.Write(f.ctx, func(db *sql.Conn) error {
				run, err := ReadAgentRun(f.ctx, db, f.turn)
				if err != nil {
					return err
				}
				expected := *run.Waiting
				switch name {
				case "both missing":
					run.Waiting.AccountIdentity, run.Waiting.AuthenticationTarget = "", ""
				case "account missing":
					run.Waiting.AccountIdentity = ""
				case "target missing":
					run.Waiting.AuthenticationTarget = ""
				case "account changed":
					run.Waiting.AuthenticationTarget = ""
					f.old.AccountIdentity = "another-account"
					expected = *run.Waiting
				case "target changed":
					run.Waiting.AccountIdentity = ""
					f.old.AuthenticationTarget = "another-target"
					expected = *run.Waiting
				}
				if err := putAgentRun(f.ctx, db, run); err != nil {
					return err
				}
				f.old.Revision++
				if err := f.publish(t, db, false); err != nil {
					return err
				}
				if _, err := SetRunCredentialWait(f.ctx, db, f.node, f.turn, &protocol.ConversationRunWaiting{Kind: "controller_login", Credential: f.previous, Provider: f.provider.ID}); err != nil {
					return err
				}
				run, err = ReadAgentRun(f.ctx, db, f.turn)
				if err != nil {
					return err
				}
				if !reflect.DeepEqual(run.Waiting, &expected) {
					return fmt.Errorf("legacy identity changed the wait or replaced trusted metadata: got %+v, want %+v", run.Waiting, expected)
				}
				if name == "account changed" || name == "target changed" {
					return nil
				}
				if err := f.publish(t, db, true); err != nil {
					return err
				}
				_, _, err = ExecuteAgentCommand(f.ctx, db, f.node, f.storage.Profile, f.controller, f.request(), time.Now())
				return err
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestControllerLoginRecoveryEnrichesWaitBeforeScheduledRefresh(t *testing.T) {
	t.Parallel()
	for _, name := range []string{"both missing", "account missing", "target missing", "preserve account", "preserve target", "identity unavailable"} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			f := newControllerLoginFixture(t)
			identity, target := f.old.AccountIdentity, f.old.AuthenticationTarget
			var waiting protocol.ConversationRunWaiting
			err := f.storage.Node.Write(f.ctx, func(db *sql.Conn) error {
				run, err := ReadAgentRun(f.ctx, db, f.turn)
				if err != nil {
					return err
				}
				switch name {
				case "account missing", "preserve target":
					run.Waiting.AccountIdentity = ""
				case "target missing", "preserve account":
					run.Waiting.AuthenticationTarget = ""
				default:
					run.Waiting.AccountIdentity, run.Waiting.AuthenticationTarget = "", ""
				}
				waiting = *run.Waiting
				if err := putAgentRun(f.ctx, db, run); err != nil {
					return err
				}
				f.old.AccountIdentity, f.old.AuthenticationTarget = "", ""
				f.old.Revision++
				return f.publish(t, db, false)
			})
			if err != nil {
				t.Fatal(err)
			}
			f.old.AccountIdentity, f.old.AuthenticationTarget = identity, target
			f.old.Revision++
			expected := ""
			switch name {
			case "preserve account":
				f.old.AccountIdentity, f.next.AccountIdentity = "another-account", "another-account"
				expected = "account_mismatch"
			case "preserve target":
				f.old.AuthenticationTarget, f.next.AuthenticationTarget = "another-target", "another-target"
				expected = "target_changed"
			case "identity unavailable":
				f.old.AccountIdentity = ""
				expected = "identity_unavailable"
			}
			// Both credentials arrive together; recovery wins the race against
			// the scheduled SetRunCredentialWait projection refresh.
			err = f.storage.Node.Write(f.ctx, func(db *sql.Conn) error {
				return f.publish(t, db, true)
			})
			if err != nil {
				t.Fatal(err)
			}
			err = f.storage.Node.Write(f.ctx, func(db *sql.Conn) error {
				_, _, err := ExecuteAgentCommand(f.ctx, db, f.node, f.storage.Profile, f.controller, f.request(), time.Now())
				if expected == "" && err != nil || controllerLoginReason(err) != expected {
					return fmt.Errorf("recovery returned %v, expected %q", err, expected)
				}
				run, err := ReadAgentRun(f.ctx, db, f.turn)
				if err != nil {
					return err
				}
				want := f.replacement
				if expected == "" {
					if run.Waiting != nil || run.Status != protocol.ConversationStatusRunning {
						return fmt.Errorf("enriched recovery did not resume the running task")
					}
				} else {
					want = f.previous
					if !reflect.DeepEqual(run.Waiting, &waiting) {
						return fmt.Errorf("rejected recovery changed the captured waiting identity")
					}
				}
				effective, err := EffectiveTurnCredential(f.ctx, db, f.turn, f.previous)
				if err != nil || effective != want {
					return fmt.Errorf("effective credential after recovery = %v, want %v: %v", effective, want, err)
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestInterruptedCredentialRenewalBecomesUncertainWithNewImmutableRevision(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		provider := providerFixture()
		provider.Authentication, provider.Api, provider.Endpoint = protocol.AuthenticationClaudeOAuth, protocol.ConversationModelApiAnthropic, "https://api.anthropic.com"
		id := protocol.NewCredentialID()
		account, workspace := "account-a", "workspace-a"
		grant := providers.AccountGrant{Kind: provider.Authentication, Access: "expired-access", Refresh: "not-exported-refresh", AccountID: &account, OrgID: &workspace,
			ExpiresAtMs: 1, RenewalPending: true}
		body, err := json.Marshal(grant)
		if err != nil {
			return err
		}
		secret := protocol.NewSecret(string(body))
		provider.Credential = &protocol.CredentialRef{Node: node, ID: id}
		if err := saveProviderConfiguration(ctx, db, providerConfiguration{ID: provider.ID, Listed: true, Provider: &provider,
			Authorizations: map[protocol.CredentialID]providerAuthorization{id: {Authentication: provider.Authentication, Revision: 4, Key: &secret}}}); err != nil {
			return err
		}
		before, err := exportRuntimeCredentials(ctx, db, node, map[protocol.CredentialID]runtimeCapture{id: {}})
		if err != nil {
			return err
		}
		if err := RecoverCredentialRenewals(ctx, db, node); err != nil {
			return err
		}
		saved, err := currentProvider(ctx, db, provider.ID, nil)
		if err != nil {
			return err
		}
		entry := saved.Authorizations[id]
		if entry.Revision != 5 {
			return fmt.Errorf("recovery credential revision=%d", entry.Revision)
		}
		if err := json.Unmarshal([]byte(entry.Key.Expose()), &grant); err != nil {
			return err
		}
		if grant.RenewalPending || !grant.RenewalUncertain {
			return fmt.Errorf("restart retried or retained pending renewal")
		}
		after, err := exportRuntimeCredentials(ctx, db, node, map[protocol.CredentialID]runtimeCapture{id: {}})
		if err != nil {
			return err
		}
		if len(before) != 1 || len(after) != 1 || after[0].Revision <= before[0].Revision {
			return fmt.Errorf("uncertain projection reused an immutable revision")
		}
		var runtime protocol.RuntimeCredential
		if err := json.Unmarshal(after[0].Body, &runtime); err != nil {
			return err
		}
		if runtime.State != "uncertain" || runtime.Revision != after[0].Revision || runtime.AccountIdentity == "" {
			return fmt.Errorf("restart lost runtime recovery metadata")
		}
		if err := RecoverCredentialRenewals(ctx, db, node); err != nil {
			return err
		}
		stable, err := exportRuntimeCredentials(ctx, db, node, map[protocol.CredentialID]runtimeCapture{id: {}})
		if err != nil {
			return err
		}
		if stable[0].Revision != after[0].Revision || stable[0].Digest != after[0].Digest {
			return fmt.Errorf("repeated startup recovery changed an already recovered credential")
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
