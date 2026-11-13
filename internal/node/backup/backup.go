// Package backup builds, encrypts, publishes and restores profile archives.
// The execution Node's storage owner supplies the consistent database copy.
package backup

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/url"
	"sort"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	MaxBytes    uint64 = 1024 * 1024 * 1024 * 1024
	MaxFiles           = 200_000
	MaxManifest        = 32 * 1024 * 1024
)

// Auth stays on the execution Node; no value reaches snapshots, events or faults.
type Auth struct {
	Identity     *protocol.Secret
	Secret       *protocol.Secret
	SessionToken *protocol.Secret
	Passphrase   *protocol.Secret
}

// Plan is one operation's captured configuration. Transport is the Node's
// outbound HTTP transport and carries its proxy settings.
type Plan struct {
	Destination   *protocol.BackupDestination
	Local         *protocol.BackupLocal
	OnlineEnabled bool
	// Keep is how many of the Node's newest backups a location retains; zero keeps all.
	Keep      uint32
	Auth      Auth
	Transport http.RoundTripper
}

// Export publishes the sealed snapshot to every enabled location.
func Export(ctx context.Context, plan Plan, packed *Packed, id string) (protocol.BackupRecord, error) {
	record := NewRecord(packed, id)
	exported := false
	for _, location := range []protocol.BackupLocation{protocol.BackupLocationLocal, protocol.BackupLocationOnline} {
		enabled := plan.Local != nil && plan.Local.Enabled
		if location == protocol.BackupLocationOnline {
			enabled = plan.OnlineEnabled && plan.Destination != nil
		}
		if enabled {
			if err := publish(ctx, plan, location, packed, record); err != nil {
				if !exported {
					return protocol.BackupRecord{}, err
				}
				failure := &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "backup storage request failed"}
				_ = errors.As(err, &failure)
				return record, &protocol.Fault{
					Code: failure.Code, Message: "backup saved to one destination; another destination failed",
					Details: &protocol.FaultDetails{Kind: "backup_partial", Data: protocol.FaultDetailsBackupPartialPayload{Record: record, Location: location, Failure: *failure}},
				}
			}
			exported = true
		}
	}
	if !exported {
		return protocol.BackupRecord{}, fault(protocol.ErrorCodeNotConfigured, "backup destination is not configured")
	}
	return record, nil
}

func NewRecord(packed *Packed, id string) protocol.BackupRecord {
	return protocol.BackupRecord{
		ID: id, Node: packed.Snapshot.Manifest.Node, CreatedMs: uint64(time.Now().UnixMilli()),
		SizeBytes: packed.Size, Files: uint32(len(packed.Snapshot.Manifest.Files)), Sha256: packed.SHA256,
		Skipped: packed.Snapshot.Manifest.SkippedTotal,
	}
}

// List merges the configured locations, newest first. A location that cannot
// be read, a record that does not read back and copies that disagree mark the
// page partial instead of hiding the backups that can be restored; the call
// fails only when no configured location answers.
func List(ctx context.Context, plan Plan) (protocol.BackupPage, error) {
	records := map[string]protocol.BackupRecord{}
	locations := map[string]protocol.BackupLocations{}
	partial := false
	var failure error
	answered := 0
	for _, location := range []protocol.BackupLocation{protocol.BackupLocationLocal, protocol.BackupLocationOnline} {
		if location == protocol.BackupLocationLocal && plan.Local == nil || location == protocol.BackupLocationOnline && plan.Destination == nil {
			continue
		}
		page, err := list(ctx, plan, location)
		if err != nil {
			if code(err) == protocol.ErrorCodeCancelled {
				return protocol.BackupPage{}, err
			}
			if failure == nil {
				failure = err
			}
			partial = true
			continue
		}
		answered++
		partial = partial || page.Partial
		for _, record := range page.Records {
			if previous, ok := records[record.ID]; ok && previous != record {
				// The first copy listed stays; the other is not offered.
				partial = true
				continue
			}
			entry := locations[record.ID]
			if location == protocol.BackupLocationLocal {
				entry.Local = true
			} else {
				entry.Online = true
			}
			locations[record.ID] = entry
			records[record.ID] = record
		}
	}
	if answered == 0 && failure != nil {
		return protocol.BackupPage{}, failure
	}
	page := protocol.BackupPage{Records: make([]protocol.BackupRecord, 0, len(records)), Partial: partial, Locations: locations}
	for _, record := range records {
		page.Records = append(page.Records, record)
	}
	sortRecords(page.Records)
	return page, nil
}

// Prune deletes the Node's own backups beyond the newest plan.Keep in every
// enabled location and returns how many it removed. Backups of other Nodes in
// a shared location, records that do not read back and every location when
// Keep is zero are left alone. A backup's commit marker goes first, so a
// half-removed backup is never listed.
func Prune(ctx context.Context, plan Plan, node protocol.NodeID) (int, error) {
	if plan.Keep == 0 {
		return 0, nil
	}
	removed := 0
	for _, location := range []protocol.BackupLocation{protocol.BackupLocationLocal, protocol.BackupLocationOnline} {
		enabled := plan.Local != nil && plan.Local.Enabled
		if location == protocol.BackupLocationOnline {
			enabled = plan.OnlineEnabled && plan.Destination != nil
		}
		if !enabled {
			continue
		}
		count, err := prune(ctx, plan, location, node)
		removed += count
		if err != nil {
			return removed, err
		}
	}
	return removed, nil
}

func sortRecords(records []protocol.BackupRecord) {
	sort.Slice(records, func(left, right int) bool {
		if records[left].CreatedMs != records[right].CreatedMs {
			return records[left].CreatedMs > records[right].CreatedMs
		}
		return records[left].ID < records[right].ID
	})
}

// ValidatePassphrase checks a newly chosen passphrase; existing archives keep theirs.
func ValidatePassphrase(value string) error {
	if utf8.RuneCountInString(value) < 8 || len(value) > 8192 || strings.ContainsFunc(value, unicode.IsControl) {
		return invalid("backup passphrase requires at least 8 characters without control characters")
	}
	return nil
}

func ValidateDestination(destination protocol.BackupDestination) error {
	endpoint, err := url.Parse(destination.Endpoint)
	if err != nil {
		return invalid("invalid backup endpoint")
	}
	if len(destination.Endpoint) > 4096 || endpoint.Scheme != "https" && endpoint.Scheme != "http" || endpoint.Hostname() == "" ||
		endpoint.User != nil || endpoint.RawQuery != "" || endpoint.ForceQuery || endpoint.Fragment != "" || strings.Contains(destination.Endpoint, "#") {
		return invalid("backup endpoint requires an HTTP URL without credentials")
	}
	if len(destination.Root) > 4096 || strings.ContainsAny(destination.Root, "\\:") || strings.ContainsFunc(destination.Root, unicode.IsControl) {
		return invalid("invalid backup storage prefix")
	}
	for _, part := range strings.Split(destination.Root, "/") {
		if part == "." || part == ".." {
			return invalid("invalid backup storage prefix")
		}
	}
	if destination.Kind == protocol.BackupKindS3 {
		bucket := func(ch rune) bool {
			return !(ch < utf8.RuneSelf && (unicode.IsLetter(ch) || unicode.IsDigit(ch)) || ch == '.' || ch == '-' || ch == '_')
		}
		region := func(ch rune) bool {
			return !(ch < utf8.RuneSelf && (unicode.IsLetter(ch) || unicode.IsDigit(ch)) || ch == '-')
		}
		if destination.Bucket == "" || len(destination.Bucket) > 255 || strings.ContainsFunc(destination.Bucket, bucket) ||
			destination.Region == "" || len(destination.Region) > 128 || strings.ContainsFunc(destination.Region, region) {
			return invalid("S3 bucket and region are required")
		}
	}
	return nil
}

// ValidIdentifier accepts only the canonical form of a backup's request identifier.
func ValidIdentifier(id string) bool {
	parsed, err := uuid.Parse(id)
	return err == nil && parsed.String() == id
}

func validateRecord(record protocol.BackupRecord) error {
	if _, err := uuid.Parse(record.ID); err != nil {
		return invalid("invalid backup identifier")
	}
	if !ValidIdentifier(record.ID) || !digest(record.Sha256) {
		return invalid("invalid backup metadata")
	}
	return nil
}

func cancelled(ctx context.Context) error {
	if ctx.Err() != nil {
		return fault(protocol.ErrorCodeCancelled, "profile backup cancelled")
	}
	return nil
}

func ioFault(err error) error {
	return fault(protocol.ErrorCodeUnavailable, fmt.Sprintf("profile backup storage failed: %v", err))
}

func invalid(message string) error { return fault(protocol.ErrorCodeInvalidRequest, message) }

func fault(code protocol.ErrorCode, message string) error {
	return &protocol.Fault{Code: code, Message: message}
}
