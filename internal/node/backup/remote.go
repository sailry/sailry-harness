package backup

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	partBytes  = 4 * 1024 * 1024
	maxRecords = 1000
)

type part struct {
	Size   uint32 `json:"size"`
	SHA256 string `json:"sha256"`
}

// remoteManifest is the commit marker `<id>.json` beside the `<id>/` segments.
type remoteManifest struct {
	Version uint32                `json:"version"`
	Record  protocol.BackupRecord `json:"record"`
	Parts   []part                `json:"parts"`
}

func (manifest remoteManifest) validate(id string) error {
	if err := validateRecord(manifest.Record); err != nil {
		return err
	}
	if manifest.Version != 1 || manifest.Record.ID != id || manifest.Record.SizeBytes > MaxBytes ||
		len(manifest.Parts) == 0 || uint64(len(manifest.Parts)) > MaxBytes/partBytes+1 {
		return invalid("invalid remote backup manifest")
	}
	var total uint64
	for index, part := range manifest.Parts {
		if part.Size == 0 || part.Size > partBytes || index+1 < len(manifest.Parts) && part.Size != partBytes || !digest(part.SHA256) {
			return invalid("invalid backup segment metadata")
		}
		total += uint64(part.Size)
	}
	if total != manifest.Record.SizeBytes {
		return invalid("backup segment sizes differ from total")
	}
	return nil
}

func publish(ctx context.Context, plan Plan, location protocol.BackupLocation, packed *Packed, record protocol.BackupRecord) error {
	if err := cancelled(ctx); err != nil {
		return err
	}
	id := record.ID
	target, err := plan.open(ctx, location)
	if err != nil {
		return err
	}
	defer target.close()
	// A request-derived directory is owned by this one export; no shared object is replaced.
	if err := step(ctx, target.createDirectory(id)); err != nil {
		return err
	}
	input, err := os.Open(packed.Path)
	if err != nil {
		return ioFault(err)
	}
	defer input.Close()
	var parts []part
	buffer := make([]byte, partBytes)
	for {
		if err := cancelled(ctx); err != nil {
			plan.cleanup(location, id, len(parts))
			return err
		}
		length, err := io.ReadFull(input, buffer)
		if err != nil && !errors.Is(err, io.EOF) && !errors.Is(err, io.ErrUnexpectedEOF) {
			return ioFault(err)
		}
		if length == 0 {
			break
		}
		if err := step(ctx, target.write(partPath(id, len(parts)), buffer[:length])); err != nil {
			plan.cleanup(location, id, len(parts)+1)
			if code(err) == protocol.ErrorCodeCancelled {
				return err
			}
			return uncertain(err)
		}
		sum := sha256.Sum256(buffer[:length])
		parts = append(parts, part{Size: uint32(length), SHA256: hex.EncodeToString(sum[:])})
	}
	manifest := remoteManifest{Version: 1, Record: record, Parts: parts}
	if err := manifest.validate(id); err != nil {
		return err
	}
	body, err := json.Marshal(manifest)
	if err != nil {
		return ioFault(err)
	}
	if len(body) > MaxManifest {
		plan.cleanup(location, id, len(parts))
		return invalid("backup manifest exceeds the limit")
	}
	// The manifest is the commit marker. An uncertain write is never repeated or deleted.
	if err := step(ctx, target.write(id+".json", body)); err != nil {
		return uncertain(err)
	}
	return nil
}

func list(ctx context.Context, plan Plan, location protocol.BackupLocation) (protocol.BackupPage, error) {
	target, err := plan.open(ctx, location)
	if err != nil {
		return protocol.BackupPage{}, err
	}
	defer target.close()
	page := protocol.BackupPage{Records: []protocol.BackupRecord{}}
	var ids []string
	inspected := 0
	err = step(ctx, target.list(func(name string, file bool) bool {
		inspected++
		if inspected > maxRecords*4 || len(ids) >= maxRecords {
			page.Partial = true
			return false
		}
		if id, ok := strings.CutSuffix(name, ".json"); ok && file && ValidIdentifier(id) {
			ids = append(ids, id)
		}
		return true
	}))
	if err != nil {
		return protocol.BackupPage{}, err
	}
	for _, id := range ids {
		manifest, err := readManifest(ctx, target, id)
		if err != nil {
			return protocol.BackupPage{}, err
		}
		page.Records = append(page.Records, manifest.Record)
	}
	sortRecords(page.Records)
	return page, nil
}

// Fetch downloads and verifies one sealed archive into a new file in temporary.
// The caller removes the returned file.
func Fetch(ctx context.Context, plan Plan, id string, location protocol.BackupLocation, temporary string) (protocol.BackupRecord, *os.File, error) {
	target, err := plan.open(ctx, location)
	if err != nil {
		return protocol.BackupRecord{}, nil, err
	}
	defer target.close()
	manifest, err := readManifest(ctx, target, id)
	if err != nil {
		return protocol.BackupRecord{}, nil, err
	}
	file, err := os.CreateTemp(temporary, "fetched-")
	if err != nil {
		return protocol.BackupRecord{}, nil, ioFault(err)
	}
	err = fetchParts(ctx, target, manifest, file)
	if err == nil {
		err = wrapIO(file.Sync())
	}
	if err == nil {
		_, seekErr := file.Seek(0, io.SeekStart)
		err = wrapIO(seekErr)
	}
	if err != nil {
		_ = file.Close()
		_ = os.Remove(file.Name())
		return protocol.BackupRecord{}, nil, err
	}
	return manifest.Record, file, nil
}

func fetchParts(ctx context.Context, target storage, manifest remoteManifest, output io.Writer) error {
	archive := sha256.New()
	for index, spec := range manifest.Parts {
		path := partPath(manifest.Record.ID, index)
		size, file, err := target.stat(path)
		if err := step(ctx, err); err != nil {
			return err
		}
		if !file || size != uint64(spec.Size) {
			return invalid("backup segment size changed")
		}
		reader, err := target.read(path)
		if err := step(ctx, err); err != nil {
			return err
		}
		segment := sha256.New()
		count, err := io.Copy(io.MultiWriter(segment, archive, output), io.LimitReader(reader, int64(spec.Size)+1))
		_ = reader.Close()
		if ctx.Err() != nil {
			return fault(protocol.ErrorCodeCancelled, "backup restore cancelled")
		}
		if err != nil {
			return fault(protocol.ErrorCodeUnavailable, "backup storage request failed")
		}
		if count > int64(spec.Size) {
			return invalid("backup segment exceeds its declared size")
		}
		if count != int64(spec.Size) || hex.EncodeToString(segment.Sum(nil)) != spec.SHA256 {
			return invalid("backup segment checksum changed")
		}
	}
	if hex.EncodeToString(archive.Sum(nil)) != manifest.Record.Sha256 {
		return invalid("backup archive checksum changed")
	}
	return nil
}

func readManifest(ctx context.Context, target storage, id string) (remoteManifest, error) {
	if !ValidIdentifier(id) {
		return remoteManifest{}, invalid("invalid backup identifier")
	}
	path := id + ".json"
	size, file, err := target.stat(path)
	if err := step(ctx, err); err != nil {
		return remoteManifest{}, err
	}
	if !file || size > MaxManifest {
		return remoteManifest{}, invalid("backup manifest exceeds the limit")
	}
	reader, err := target.read(path)
	if err := step(ctx, err); err != nil {
		return remoteManifest{}, err
	}
	body, err := io.ReadAll(io.LimitReader(reader, MaxManifest+1))
	_ = reader.Close()
	if ctx.Err() != nil {
		return remoteManifest{}, fault(protocol.ErrorCodeCancelled, "backup read cancelled")
	}
	if err != nil {
		return remoteManifest{}, fault(protocol.ErrorCodeUnavailable, "backup storage request failed")
	}
	if len(body) > MaxManifest {
		return remoteManifest{}, invalid("backup manifest exceeds the limit")
	}
	var manifest remoteManifest
	decoder := json.NewDecoder(bytes.NewReader(body))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&manifest); err != nil {
		return remoteManifest{}, invalid("invalid remote backup manifest")
	}
	return manifest, manifest.validate(id)
}

// cleanup removes the segments of a failed export within ten seconds.
func (plan Plan) cleanup(location protocol.BackupLocation, id string, parts int) {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	target, err := plan.open(ctx, location)
	if err != nil {
		return
	}
	defer target.close()
	for index := range parts {
		_ = target.remove(partPath(id, index))
	}
	_ = target.removeDirectory(id)
}

func partPath(id string, index int) string { return fmt.Sprintf("%s/%06d.bin", id, index) }

// step maps one storage result. Library diagnostics may contain signed URLs or
// credentials, so they never leave this package.
func step(ctx context.Context, err error) error {
	if ctx.Err() != nil {
		return fault(protocol.ErrorCodeCancelled, "profile backup cancelled")
	}
	if err == nil {
		return nil
	}
	var failure *protocol.Fault
	if errors.As(err, &failure) {
		return failure
	}
	return fault(protocol.ErrorCodeUnavailable, "backup storage request failed")
}

func status(code int) error {
	switch code {
	case 401, 403:
		return fault(protocol.ErrorCodePermissionDenied, "backup storage request failed")
	case 404:
		return fault(protocol.ErrorCodeNotFound, "backup storage request failed")
	}
	return fault(protocol.ErrorCodeUnavailable, "backup storage request failed")
}

func code(err error) protocol.ErrorCode {
	var failure *protocol.Fault
	if errors.As(err, &failure) {
		return failure.Code
	}
	return ""
}

func uncertain(err error) error {
	if value := code(err); value == protocol.ErrorCodePermissionDenied || value == protocol.ErrorCodeInvalidRequest {
		return err
	}
	return fault(protocol.ErrorCodeOutcomeUnknown, "backup export outcome is unknown; inspect the remote backup list")
}

// storage is one location's flat layout: `<id>.json` and `<id>/<index>.bin`.
type storage interface {
	createDirectory(name string) error
	write(path string, data []byte) error
	list(visit func(name string, file bool) bool) error
	stat(path string) (size uint64, file bool, err error)
	read(path string) (io.ReadCloser, error)
	remove(path string) error
	removeDirectory(name string) error
	close()
}

func (plan Plan) open(ctx context.Context, location protocol.BackupLocation) (storage, error) {
	if location == protocol.BackupLocationLocal {
		if plan.Local == nil {
			return nil, fault(protocol.ErrorCodeNotConfigured, "local backup destination is not configured")
		}
		if !filepath.IsAbs(plan.Local.Directory) {
			return nil, invalid("local backup directory must be absolute")
		}
		if err := os.MkdirAll(plan.Local.Directory, 0o700); err != nil {
			return nil, ioFault(err)
		}
		root, err := os.OpenRoot(plan.Local.Directory)
		if err != nil {
			return nil, ioFault(err)
		}
		return localStorage{root: root}, nil
	}
	if plan.Destination == nil {
		return nil, fault(protocol.ErrorCodeNotConfigured, "online backup destination is not configured")
	}
	if err := ValidateDestination(*plan.Destination); err != nil {
		return nil, err
	}
	if plan.Transport == nil {
		return nil, fault(protocol.ErrorCodeNotConfigured, "online backup transport is not configured")
	}
	identity, secret := expose(plan.Auth.Identity), expose(plan.Auth.Secret)
	transport := scopedTransport{ctx: ctx, base: plan.Transport}
	prefix := strings.Trim(plan.Destination.Root, "/")
	if plan.Destination.Kind == protocol.BackupKindS3 {
		if identity == "" || secret == "" {
			return nil, fault(protocol.ErrorCodeNotConfigured, "S3 credentials are missing")
		}
		return openS3(ctx, *plan.Destination, prefix, identity, secret, expose(plan.Auth.SessionToken), transport)
	}
	return openWebDAV(plan.Destination.Endpoint, prefix, identity, secret, transport), nil
}

func expose(value *protocol.Secret) string {
	if value == nil {
		return ""
	}
	return value.Expose()
}

type localStorage struct{ root *os.Root }

func (target localStorage) createDirectory(name string) error {
	if err := target.root.Mkdir(name, 0o700); err != nil && !errors.Is(err, os.ErrExist) {
		return localFault(err)
	}
	return nil
}

func (target localStorage) write(path string, data []byte) error {
	file, err := target.root.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_TRUNC, 0o600)
	if err != nil {
		return localFault(err)
	}
	_, err = file.Write(data)
	return localFault(errors.Join(err, file.Sync(), file.Close()))
}

func (target localStorage) list(visit func(string, bool) bool) error {
	directory, err := target.root.Open(".")
	if err != nil {
		return localFault(err)
	}
	defer directory.Close()
	for {
		items, err := directory.ReadDir(256)
		for _, item := range items {
			if !visit(item.Name(), item.Type().IsRegular()) {
				return nil
			}
		}
		if errors.Is(err, io.EOF) {
			return nil
		}
		if err != nil {
			return localFault(err)
		}
	}
}

func (target localStorage) stat(path string) (uint64, bool, error) {
	info, err := target.root.Stat(path)
	if err != nil {
		return 0, false, localFault(err)
	}
	return uint64(info.Size()), info.Mode().IsRegular(), nil
}

func (target localStorage) read(path string) (io.ReadCloser, error) {
	file, err := target.root.Open(path)
	return file, localFault(err)
}

func (target localStorage) remove(path string) error { return localFault(target.root.Remove(path)) }

func (target localStorage) removeDirectory(name string) error {
	return localFault(target.root.Remove(name))
}

func (target localStorage) close() { _ = target.root.Close() }

func localFault(err error) error {
	switch {
	case err == nil:
		return nil
	case errors.Is(err, os.ErrNotExist):
		return status(404)
	case errors.Is(err, os.ErrPermission):
		return status(403)
	}
	return status(0)
}
