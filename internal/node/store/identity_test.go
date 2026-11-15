package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func identityTestAddress(device protocol.NodeID) string {
	body, _ := json.Marshal(link.EndpointAddress{ID: device, Addrs: []json.RawMessage{}})
	return string(body)
}

func TestIdentitySponsorshipRequiresTargetAndMergesWholeUser(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		sponsor, device, sibling := protocol.NodeID{2}, protocol.NodeID{3}, protocol.NodeID{4}
		for _, peer := range []protocol.NodeID{sponsor, device} {
			if err := PairIdentityPeer(ctx, db, node, peer, link.DirectionController, link.InvitationScope{Kind: protocol.InvitationOtherUser}); err != nil {
				return err
			}
		}
		target, err := CallerUser(ctx, db, node, sponsor)
		if err != nil {
			return err
		}
		source, err := CallerUser(ctx, db, node, device)
		if err != nil {
			return err
		}
		if err := PairIdentityPeer(ctx, db, node, sibling, link.DirectionController, link.InvitationScope{Kind: protocol.InvitationMyDevice, User: &source.ID}); err != nil {
			return err
		}
		project := protocol.NewProjectID()
		if _, err := db.ExecContext(ctx, "INSERT INTO projects(id,name,path,appearance,owner) VALUES(?1,'Owned project','/identity-test','{}',?2)", project.String(), source.ID.String()); err != nil {
			return err
		}
		info := pluginTestInfo("identity-records", 'a')
		var reference protocol.PluginReference
		for _, user := range []protocol.User{source, target} {
			owner := WithUser(ctx, user)
			installed, err := InstallPlugin(owner, db, info, 0, true)
			if err != nil {
				return err
			}
			reference = protocol.PluginReference{Name: installed.Summary.Name, Digest: installed.Summary.Digest, SettingsRevision: installed.Summary.SettingsRevision}
			value := json.RawMessage(`"source"`)
			if user.ID == target.ID {
				value = json.RawMessage(`"target"`)
			}
			if _, err := WritePluginValue(owner, db, reference, "conflict", value, 0, nil); err != nil {
				return err
			}
		}
		if _, err := WritePluginValue(WithUser(ctx, source), db, reference, "promoted", json.RawMessage(`"kept"`), 0, nil); err != nil {
			return err
		}
		request := protocol.NewRequestID()
		pending, err := SponsorIdentityDevice(ctx, db, node, sponsor, request, protocol.SponsorDevice{Device: device, Address: identityTestAddress(device), Generation: 1})
		if err != nil || pending.State != "pending" {
			return fmt.Errorf("expected pending proposal: %s %v", pending.State, err)
		}
		before, err := CallerUser(ctx, db, node, device)
		if err != nil || before.ID != source.ID {
			return errors.New("sponsorship transferred membership before target confirmation")
		}
		if _, err := ConfirmIdentitySponsorship(ctx, db, node, sponsor, protocol.ConfirmSponsorship{Sponsor: sponsor, Request: request}); err == nil {
			return errors.New("sponsor confirmed on target's behalf")
		}
		if err := RevokeIdentityDevice(ctx, db, node, sponsor, protocol.RevokeDevice{Device: device, Sponsored: true, Generation: 1, ConfirmLast: true}); err != nil {
			return err
		}
		if _, err := ConfirmIdentitySponsorship(ctx, db, node, device, protocol.ConfirmSponsorship{Sponsor: sponsor, Request: request}); err == nil {
			return errors.New("cancelled pending proposal still merged the target")
		}
		request = protocol.NewRequestID()
		pending, err = SponsorIdentityDevice(ctx, db, node, sponsor, request, protocol.SponsorDevice{Device: device, Address: identityTestAddress(device), Generation: 2})
		if err != nil || pending.State != "pending" {
			return fmt.Errorf("new proposal: %s %v", pending.State, err)
		}
		if _, err := ConfirmIdentitySponsorship(ctx, db, node, device, protocol.ConfirmSponsorship{Sponsor: sponsor, Request: request}); err != nil {
			return err
		}
		for _, peer := range []protocol.NodeID{device, sibling} {
			user, err := CallerUser(ctx, db, node, peer)
			if err != nil || user.ID != target.ID {
				return fmt.Errorf("whole group membership did not move: %v", err)
			}
		}
		var owner string
		if err := db.QueryRowContext(ctx, "SELECT owner FROM projects WHERE id=?1", project.String()).Scan(&owner); err != nil || owner != target.ID.String() {
			return errors.New("owned project did not move with its devices")
		}
		resolved, err := ResolveUser(ctx, db, source.ID)
		if err != nil || resolved.ID != target.ID {
			return errors.New("source did not resolve to surviving user")
		}
		for _, test := range []struct {
			context   context.Context
			key, want string
		}{
			{WithUser(ctx, target), "conflict", `"target"`},
			{WithUser(ctx, target), "promoted", `"kept"`},
			{WithCapturedUser(WithUser(ctx, target), source.ID), "conflict", `"source"`},
		} {
			value, err := ReadPluginValue(test.context, db, reference, test.key)
			if err != nil || string(value.Value) != test.want {
				return fmt.Errorf("merge value %s: %s %v", test.key, value.Value, err)
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestIdentityKnownMyDevicePairingRequiresSeparateConfirmation(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		device := protocol.NodeID{2}
		if err := PairIdentityPeer(ctx, db, node, device, link.DirectionController, link.InvitationScope{Kind: protocol.InvitationOtherUser}); err != nil {
			return err
		}
		original, err := CallerUser(ctx, db, node, device)
		if err != nil {
			return err
		}
		local, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		for range 2 {
			if err := PairIdentityPeer(ctx, db, node, device, link.DirectionController, link.InvitationScope{Kind: protocol.InvitationMyDevice}); err != nil {
				return err
			}
		}
		current, err := CallerUser(ctx, db, node, device)
		if err != nil || current.ID != original.ID {
			return errors.New("repeat pairing reassigned known membership")
		}
		state, err := ReadIdentity(ctx, db, node, device)
		if err != nil || len(state.Sponsorships) != 1 {
			return fmt.Errorf("known-device proposal was missing or duplicated: %d %v", len(state.Sponsorships), err)
		}
		proposal := state.Sponsorships[0]
		if proposal.Sponsor != node || proposal.User != local.ID {
			return errors.New("proposal did not retain sharer's captured user")
		}
		if _, err := ConfirmIdentitySponsorship(ctx, db, node, device, protocol.ConfirmSponsorship{Sponsor: node, Request: proposal.Request}); err != nil {
			return err
		}
		current, err = CallerUser(ctx, db, node, device)
		if err != nil || current.ID != local.ID {
			return errors.New("confirmed my-device pairing did not join sharer")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestIdentityRevocationEpochsRejectDelayedEffects(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		sponsor, device := protocol.NodeID{2}, protocol.NodeID{3}
		if err := PairIdentityPeer(ctx, db, node, sponsor, link.DirectionController, link.InvitationScope{Kind: protocol.InvitationOtherUser}); err != nil {
			return err
		}
		revoke := func(generation uint64) error {
			return RevokeIdentityDevice(ctx, db, node, sponsor, protocol.RevokeDevice{Device: device, Sponsored: true, Generation: generation, ConfirmLast: true})
		}
		grant := func(generation uint64) (protocol.DeviceSponsorship, error) {
			return SponsorIdentityDevice(ctx, db, node, sponsor, protocol.NewRequestID(), protocol.SponsorDevice{Device: device, Address: identityTestAddress(device), Generation: generation})
		}
		if err := revoke(1); err != nil {
			return err
		}
		if value, err := grant(1); err != nil || value.State != "cancelled" {
			return fmt.Errorf("delayed grant survived revocation: %s %v", value.State, err)
		}
		if _, err := CallerUser(ctx, db, node, device); err == nil {
			return errors.New("revoked delayed grant gained access")
		}
		if value, err := grant(2); err != nil || value.State != "accepted" {
			return fmt.Errorf("new explicit generation failed: %s %v", value.State, err)
		}
		if err := revoke(1); err != nil {
			return err
		}
		if _, err := CallerUser(ctx, db, node, device); err != nil {
			return errors.New("old revocation removed a newer grant")
		}
		if err := revoke(2); err != nil {
			return err
		}
		if _, err := CallerUser(ctx, db, node, device); err == nil {
			return errors.New("current revocation retained access")
		}
		if value, err := grant(3); err != nil || value.State != "accepted" {
			return fmt.Errorf("source revoke created a local-removal tombstone: %s %v", value.State, err)
		}
		if err := RevokeIdentityDevice(ctx, db, node, node, protocol.RevokeDevice{Device: device}); err != nil {
			return err
		}
		if value, err := grant(4); err != nil || value.State != "cancelled" {
			return fmt.Errorf("automatic sponsor undid explicit local removal: %s %v", value.State, err)
		}
		if err := RevokeIdentityDevice(ctx, db, node, sponsor, protocol.RevokeDevice{Device: sponsor}); err == nil {
			return errors.New("last device removal lacked explicit confirmation")
		}
		if err := RevokeIdentityDevice(ctx, db, node, sponsor, protocol.RevokeDevice{Device: sponsor, ConfirmLast: true}); err != nil {
			return err
		}
		if err := RevokeIdentityDevice(ctx, db, node, node, protocol.RevokeDevice{Device: node, ConfirmLast: true}); err == nil {
			return errors.New("built-in local device was removable")
		}
		users, err := ListUsers(ctx, db)
		if err != nil || len(users) != 2 {
			return errors.New("revocation deleted orphaned user data")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestIdentityOfflineQueueKeepsRequestsAcrossReopen(t *testing.T) {
	t.Parallel()
	profile, node, storage := schemaFixture(t)
	ctx := t.Context()
	device, host := protocol.NodeID{2}, link.EndpointAddress{ID: protocol.NodeID{3}, Addrs: []json.RawMessage{}}
	var original protocol.Request
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if err := PairIdentityPeer(ctx, db, node, host.ID, link.DirectionHost, link.InvitationScope{Kind: protocol.InvitationOtherUser}); err != nil {
			return err
		}
		if err := RememberLinkPeer(ctx, db, host); err != nil {
			return err
		}
		if err := PairIdentityPeer(ctx, db, node, device, link.DirectionController, link.InvitationScope{Kind: protocol.InvitationMyDevice}); err != nil {
			return err
		}
		var address link.EndpointAddress
		if err := json.Unmarshal([]byte(identityTestAddress(device)), &address); err != nil {
			return err
		}
		if err := RememberLinkPeer(ctx, db, address); err != nil {
			return err
		}
		if err := QueueIdentitySponsors(ctx, db, node, []link.EndpointAddress{host}); err != nil {
			return err
		}
		values, err := IdentityDeliveries(ctx, db)
		if err != nil || len(values) != 1 {
			return fmt.Errorf("queued sponsor: %d %v", len(values), err)
		}
		original = values[0].Request
		_, err = MarkIdentityDelivery(ctx, db, original.ID, "unknown", nil)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.Close(); err != nil {
		t.Fatal(err)
	}
	storage = reopenSchemaFixture(t, profile, node)
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if err := QueueIdentitySponsors(ctx, db, node, []link.EndpointAddress{host}); err != nil {
			return err
		}
		values, err := IdentityDeliveries(ctx, db)
		if err != nil || len(values) != 1 {
			return fmt.Errorf("queue duplicated after reopen: %d %v", len(values), err)
		}
		before, _ := json.Marshal(original)
		after, _ := json.Marshal(values[0].Request)
		if string(before) != string(after) || values[0].State != "unknown" {
			return errors.New("offline recovery changed the original request")
		}
		if err := RevokeIdentityDevice(ctx, db, node, node, protocol.RevokeDevice{Device: device}); err != nil {
			return err
		}
		values, err = IdentityDeliveries(ctx, db)
		if err != nil || len(values) != 1 {
			return fmt.Errorf("negative queue: %d %v", len(values), err)
		}
		if values[0].Request.ID == original.ID {
			return errors.New("revocation reused the grant's request identity")
		}
		if err := PairIdentityPeer(ctx, db, node, device, link.DirectionController, link.InvitationScope{Kind: protocol.InvitationMyDevice}); err != nil {
			return err
		}
		var address link.EndpointAddress
		if err := json.Unmarshal([]byte(identityTestAddress(device)), &address); err != nil {
			return err
		}
		if err := RememberLinkPeer(ctx, db, address); err != nil {
			return err
		}
		if err := QueueIdentitySponsors(ctx, db, node, []link.EndpointAddress{host}); err != nil {
			return err
		}
		values, err = IdentityDeliveries(ctx, db)
		if err != nil || len(values) != 2 {
			return fmt.Errorf("new generation queue: %d %v", len(values), err)
		}
		action, err := identityAction(values[1].Request.Command)
		if err != nil {
			return err
		}
		grant, err := identityPayload[protocol.SponsorDevice](action.Data)
		if err != nil || grant.Generation != 2 {
			return errors.New("explicit repaired membership did not advance its generation")
		}
		if changed, err := MarkIdentityDelivery(ctx, db, original.ID, "completed", nil); err != nil || changed {
			return errors.New("late completion resurrected cancelled grant")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestIdentityDirectoryIsPrivateAndPreservesNodeDirectPairs(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		device, other, source, direct, imported := protocol.NodeID{2}, protocol.NodeID{3}, protocol.NodeID{4}, protocol.NodeID{5}, protocol.NodeID{6}
		for _, test := range []struct {
			id        protocol.NodeID
			direction link.PeerDirection
			kind      protocol.InvitationKind
		}{
			{device, link.DirectionController, protocol.InvitationMyDevice}, {other, link.DirectionController, protocol.InvitationOtherUser}, {source, link.DirectionHost, protocol.InvitationMyDevice}, {direct, link.DirectionHost, protocol.InvitationOtherUser},
		} {
			if err := PairIdentityPeer(ctx, db, node, test.id, test.direction, link.InvitationScope{Kind: test.kind}); err != nil {
				return err
			}
		}
		if err := SaveIdentityDirectory(ctx, db, []protocol.DirectoryHost{{Node: node, Address: identityTestAddress(node)}, {Node: direct, Address: identityTestAddress(direct)}}); err != nil {
			return err
		}
		if _, err := ReadIdentityDirectory(ctx, db, node, other); err == nil {
			return errors.New("another user's administrator read source directory")
		}
		if directory, err := ReadIdentityDirectory(ctx, db, node, device); err != nil || len(directory.Hosts) != 2 {
			return fmt.Errorf("my-device directory: %d %v", len(directory.Hosts), err)
		}
		addresses := []link.EndpointAddress{{ID: direct}, {ID: imported}, {ID: device}}
		if err := ApplyIdentityDirectory(ctx, db, source, 1, addresses); err != nil {
			return err
		}
		if err := RemoveIdentityPeer(ctx, db, imported); err != nil {
			return err
		}
		if err := ApplyIdentityDirectory(ctx, db, source, 2, addresses); err != nil {
			return err
		}
		if err := RemoveIdentityPeer(ctx, db, source); err != nil {
			return err
		}
		peers, err := LinkPeers(ctx, db)
		if err != nil {
			return err
		}
		found := map[protocol.NodeID]bool{}
		for _, peer := range peers {
			found[peer] = true
		}
		if !found[direct] || found[imported] || found[source] {
			return errors.New("source cleanup lost direct trust or restored removed peer")
		}
		directions, err := LinkDirections(ctx, db)
		if err != nil {
			return err
		}
		for _, value := range directions {
			if value.ID == device && value.Direction != link.DirectionController {
				return errors.New("directory cleanup removed independent inbound membership")
			}
		}
		relationships, err := IdentityPeerRelationships(ctx, db)
		if err != nil {
			return err
		}
		for _, value := range relationships {
			if value.ID == device && value.DirectDirection.AllowsOutbound() {
				return errors.New("imported outbound became a direct host origin")
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestIdentitySponsoredRevocationPreservesIndependentDirections(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		sponsor, device := protocol.NodeID{2}, protocol.NodeID{3}
		if err := PairIdentityPeer(ctx, db, node, sponsor, link.DirectionController, link.InvitationScope{Kind: protocol.InvitationOtherUser}); err != nil {
			return err
		}
		user, err := CallerUser(ctx, db, node, sponsor)
		if err != nil {
			return err
		}
		if err := PairIdentityPeer(ctx, db, node, device, link.DirectionHost, link.InvitationScope{Kind: protocol.InvitationOtherUser}); err != nil {
			return err
		}
		if _, err := SponsorIdentityDevice(ctx, db, node, sponsor, protocol.NewRequestID(), protocol.SponsorDevice{Device: device, Address: identityTestAddress(device), Generation: 1}); err != nil {
			return err
		}
		if err := RevokeIdentityDevice(ctx, db, node, sponsor, protocol.RevokeDevice{Device: device, Sponsored: true, Generation: 1, ConfirmLast: true}); err != nil {
			return err
		}
		if _, err := CallerUser(ctx, db, node, device); err == nil {
			return errors.New("sponsored inbound revocation retained membership")
		}
		directions, err := LinkDirections(ctx, db)
		if err != nil {
			return err
		}
		for _, value := range directions {
			if value.ID == device && value.Direction != link.DirectionHost {
				return errors.New("sponsored revoke deleted independent outbound direction")
			}
		}
		if err := PairIdentityPeer(ctx, db, node, device, link.DirectionController, link.InvitationScope{Kind: protocol.InvitationMyDevice, User: &user.ID}); err != nil {
			return err
		}
		if _, err := SponsorIdentityDevice(ctx, db, node, sponsor, protocol.NewRequestID(), protocol.SponsorDevice{Device: device, Address: identityTestAddress(device), Generation: 2}); err != nil {
			return err
		}
		if err := RevokeIdentityDevice(ctx, db, node, sponsor, protocol.RevokeDevice{Device: device, Sponsored: true, Generation: 2, ConfirmLast: true}); err != nil {
			return err
		}
		current, err := CallerUser(ctx, db, node, device)
		if err != nil || current.ID != user.ID {
			return errors.New("source claimed and revoked an independently established membership")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestIdentityLocalMergePreservesLegacyPersonalSettings(t *testing.T) {
	t.Parallel()
	_, _, storage := schemaFixture(t)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		source, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		target, err := CreateUser(ctx, db, "Surviving user")
		if err != nil {
			return err
		}
		for _, value := range []struct {
			user    protocol.User
			percent uint8
		}{{source, 12}, {target, 88}} {
			settings := protocol.DefaultConversationCompactionSettings()
			settings.Percent = value.percent
			if _, _, err := saveCompaction(WithUser(ctx, value.user), db, settings); err != nil {
				return err
			}
		}
		merged, err := MergeIdentityUsers(ctx, db, source.ID, target.ID)
		if err != nil {
			return err
		}
		if !merged.Local || !merged.Administrator {
			return errors.New("local administrator invariant was lost")
		}
		settings, err := ReadCompaction(WithUser(ctx, merged), db)
		if err != nil || settings.Percent != 88 {
			return fmt.Errorf("source singleton replaced target settings: %d %v", settings.Percent, err)
		}
		settings.Percent = 44
		if _, _, err := saveCompaction(WithUser(ctx, merged), db, settings); err != nil {
			return err
		}
		var legacy []byte
		if err := db.QueryRowContext(ctx, "SELECT body FROM compaction_settings WHERE singleton=1").Scan(&legacy); err != nil {
			return err
		}
		var original protocol.ConversationCompactionSettings
		if json.Unmarshal(legacy, &original) != nil || original.Percent != 12 {
			return errors.New("surviving local user rewrote archived legacy singleton")
		}
		fixed, err := LegacyStorageUser(ctx, db)
		if err != nil || fixed != source.ID {
			return errors.New("legacy namespace moved with local flag")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
