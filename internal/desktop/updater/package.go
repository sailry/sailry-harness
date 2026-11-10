package updater

import (
	"archive/zip"
	"bytes"
	"crypto"
	"crypto/ed25519"
	"crypto/sha256"
	"crypto/sha512"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"encoding/xml"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path"
	"path/filepath"
	"runtime"
	"strings"

	"github.com/Masterminds/semver/v3"
)

const maxExtracted = 8 * 1024 * 1024 * 1024

// signatureMagic opens a zipsign-signed archive: the ed25519ph signatures
// precede the ZIP data they sign.
var signatureMagic = []byte("\x0c\x04\x01ed25519ph\x00\x00")

// verifyArchive proves the staged archive is the signed catalog's package and
// was signed by a trusted publisher.
func verifyArchive(archive string, release Release, keys []ed25519.PublicKey) error {
	if len(keys) == 0 {
		return failure("updates_source_unready", "archive verification requires trusted publisher keys")
	}
	info, err := os.Stat(archive)
	if err != nil {
		return ioFailure(err)
	}
	if filepath.Base(archive) != release.Name || uint64(info.Size()) != release.Size {
		return failure("updates_package_invalid", "the staged archive name or size differs from signed metadata")
	}
	file, err := os.Open(archive)
	if err != nil {
		return ioFailure(err)
	}
	defer file.Close()
	digest := sha256.New()
	copied, err := io.Copy(digest, io.LimitReader(file, int64(release.Size)+1))
	if err != nil {
		return ioFailure(err)
	}
	if uint64(copied) != release.Size || hex.EncodeToString(digest.Sum(nil)) != release.SHA256 {
		return failure("updates_digest_invalid", "the update archive checksum does not match")
	}
	if !verifySignature(file, release.Name, keys) {
		return failure("updates_signature_invalid", "the update archive was not signed by a trusted publisher")
	}
	reader, err := zip.NewReader(file, info.Size())
	if err != nil {
		return failure("updates_package_invalid", "the update archive is not a ZIP package")
	}
	var size uint64
	for _, entry := range reader.File {
		if !enclosed(entry.Name) {
			return failure("updates_package_invalid", "the ZIP contains an unsafe path")
		}
		size += entry.UncompressedSize64
		if size < entry.UncompressedSize64 || size > maxExtracted {
			return failure("updates_package_invalid", "the uncompressed update exceeds its size limit")
		}
	}
	return nil
}

// verifySignature checks the archive's leading signatures over the SHA-512
// prehash of everything after them, bound to the archive's file name.
func verifySignature(file *os.File, name string, keys []ed25519.PublicKey) bool {
	if _, err := file.Seek(0, io.SeekStart); err != nil {
		return false
	}
	header := make([]byte, len(signatureMagic)+2)
	if _, err := io.ReadFull(file, header); err != nil || !bytes.Equal(header[:len(signatureMagic)], signatureMagic) {
		return false
	}
	count := int(binary.LittleEndian.Uint16(header[len(signatureMagic):]))
	if count == 0 || count*ed25519.SignatureSize > 1<<16 {
		return false
	}
	signatures := make([]byte, count*ed25519.SignatureSize)
	if _, err := io.ReadFull(file, signatures); err != nil {
		return false
	}
	prehash := sha512.New()
	if _, err := io.Copy(prehash, file); err != nil {
		return false
	}
	message := prehash.Sum(nil)
	options := &ed25519.Options{Hash: crypto.SHA512, Context: name}
	for _, key := range keys {
		for i := 0; i < count; i++ {
			if ed25519.VerifyWithOptions(key, message, signatures[i*ed25519.SignatureSize:(i+1)*ed25519.SignatureSize], options) == nil {
				return true
			}
		}
	}
	return false
}

// enclosed reports whether a ZIP entry stays inside its destination.
func enclosed(name string) bool {
	name = strings.TrimSuffix(name, "/")
	return !strings.ContainsAny(name, "\\\x00") && filepath.IsLocal(filepath.FromSlash(name)) && name != "."
}

// extract unpacks the verified archive into destination and validates the
// bundle. os.Root refuses every entry that would leave the destination,
// through a symbolic link included.
func extract(archive string, release Release, destination string) (string, error) {
	rejected := failure("updates_package_invalid", "the update package could not be extracted")
	reader, err := zip.OpenReader(archive)
	if err != nil {
		return "", rejected
	}
	defer reader.Close()
	root, err := os.OpenRoot(destination)
	if err != nil {
		return "", rejected
	}
	defer root.Close()
	for _, entry := range reader.File {
		if !enclosed(entry.Name) || extractEntry(root, entry) != nil {
			return "", rejected
		}
	}
	bundle := filepath.Join(destination, release.Bundle)
	return bundle, validate(bundle, release)
}

func extractEntry(root *os.Root, entry *zip.File) error {
	name := filepath.FromSlash(strings.TrimSuffix(entry.Name, "/"))
	mode := entry.Mode()
	if mode.IsDir() {
		return root.MkdirAll(name, 0o755)
	}
	if parent := filepath.Dir(name); parent != "." {
		if err := root.MkdirAll(parent, 0o755); err != nil {
			return err
		}
	}
	source, err := entry.Open()
	if err != nil {
		return err
	}
	defer source.Close()
	if mode&os.ModeSymlink != 0 && runtime.GOOS != "windows" {
		target, err := io.ReadAll(io.LimitReader(source, 4096))
		if err != nil {
			return err
		}
		// A relative target resolves against the link's directory.
		if path.IsAbs(string(target)) || !filepath.IsLocal(filepath.Join(filepath.Dir(name), filepath.FromSlash(string(target)))) {
			return fmt.Errorf("symbolic link %q leaves the package", entry.Name)
		}
		return root.Symlink(string(target), name)
	}
	if !mode.IsRegular() {
		return fmt.Errorf("unsupported entry %q", entry.Name)
	}
	permissions := mode.Perm()
	if permissions == 0 {
		permissions = 0o644
	}
	file, err := root.OpenFile(name, os.O_WRONLY|os.O_CREATE|os.O_EXCL, permissions|0o200)
	if err != nil {
		return err
	}
	_, err = io.Copy(file, source)
	if closeErr := file.Close(); err == nil {
		err = closeErr
	}
	return err
}

type build struct {
	Version            uint32 `json:"version"`
	Target             string `json:"target"`
	ApplicationVersion string `json:"application_version"`
}

// validate checks an extracted or installed bundle against the signed
// release: its executable's architecture, build metadata and, on macOS, its
// plist and code signature.
func validate(root string, release Release) error {
	if info, err := os.Stat(root); err != nil || !info.IsDir() {
		return failure("updates_package_invalid", "the update has no application bundle")
	}
	executable, err := confined(root, filepath.Join(root, filepath.FromSlash(release.Executable)))
	if err != nil {
		return err
	}
	if err := verifyExecutable(executable, release.Target); err != nil {
		return err
	}
	macos := strings.HasSuffix(release.Target, "apple-darwin")
	resources := root
	if macos {
		resources = filepath.Join(root, "Contents", "Resources")
	}
	metadata, err := confined(root, filepath.Join(resources, "build.json"))
	if err != nil {
		return err
	}
	data, err := readLimited(metadata, 1024*1024)
	if err != nil {
		return err
	}
	var packaged build
	if err := json.Unmarshal(data, &packaged); err != nil {
		return failure("updates_package_invalid", "the packaged build metadata is invalid")
	}
	if packaged.Version != 1 || packaged.Target != release.Target || packaged.ApplicationVersion != release.Version {
		return failure("updates_package_invalid", "the packaged build metadata differs from signed update metadata")
	}
	if !macos {
		return nil
	}
	if err := validatePlist(root, release); err != nil {
		return err
	}
	if runtime.GOOS == "darwin" {
		return verifyCodeSignature(root)
	}
	return nil
}

// verifyCodeSignature is replaced by tests, whose fixture bundles are unsigned.
var verifyCodeSignature = func(root string) error {
	if err := exec.Command("/usr/bin/codesign", "--verify", "--deep", "--strict", root).Run(); err != nil {
		if _, exited := err.(*exec.ExitError); exited {
			return failure("updates_signature_invalid", "the packaged macOS code signature is invalid")
		}
		return failure("updates_signature_invalid", "the macOS code signature could not be verified")
	}
	return nil
}

func validatePlist(root string, release Release) error {
	version, err := semver.StrictNewVersion(release.Version)
	if err != nil {
		return failure("updates_package_invalid", "invalid application version")
	}
	plist, err := confined(root, filepath.Join(root, "Contents", "Info.plist"))
	if err != nil {
		return err
	}
	data, err := readLimited(plist, 128*1024)
	if err != nil {
		return err
	}
	values, err := plistStrings(data)
	if err != nil {
		return failure("updates_package_invalid", "the application plist is invalid")
	}
	if values["CFBundleIdentifier"] != "ai.sailry.desktop" || values["CFBundleExecutable"] != "sailry-desktop" ||
		values["CFBundleShortVersionString"] != fmt.Sprintf("%d.%d.%d", version.Major(), version.Minor(), version.Patch()) ||
		values["LSMinimumSystemVersion"] != release.MinimumSystem {
		return failure("updates_package_invalid", "the application plist differs from signed update metadata")
	}
	return nil
}

// plistStrings reads the top dictionary's string values of an XML plist. The
// decoder never fetches the DOCTYPE's external DTD.
func plistStrings(data []byte) (map[string]string, error) {
	decoder := xml.NewDecoder(bytes.NewReader(data))
	values := map[string]string{}
	var stack []string
	var key, text string
	pending := false
	for {
		token, err := decoder.Token()
		if err == io.EOF {
			break
		}
		if err != nil {
			return nil, err
		}
		switch t := token.(type) {
		case xml.StartElement:
			stack = append(stack, t.Name.Local)
			text = ""
			// Only the element directly after a key is its value.
			if len(stack) == 3 && t.Name.Local != "key" && t.Name.Local != "string" {
				pending = false
			}
		case xml.CharData:
			text += string(t)
		case xml.EndElement:
			if len(stack) == 3 && stack[0] == "plist" && stack[1] == "dict" {
				switch t.Name.Local {
				case "key":
					key, pending = text, true
				case "string":
					if _, seen := values[key]; pending && !seen {
						values[key] = text
					}
					pending = false
				}
			}
			stack = stack[:len(stack)-1]
		}
	}
	if len(stack) != 0 {
		return nil, io.ErrUnexpectedEOF
	}
	return values, nil
}

// confined resolves path and requires it to stay inside root.
func confined(root, path string) (string, error) {
	root, err := filepath.EvalSymlinks(root)
	if err != nil {
		return "", ioFailure(err)
	}
	path, err = filepath.EvalSymlinks(path)
	if err != nil {
		return "", failure("updates_package_invalid", "a required bundled resource is missing")
	}
	if relative, err := filepath.Rel(root, path); err != nil || !filepath.IsLocal(relative) {
		return "", failure("updates_package_invalid", "a bundled resource points outside the application")
	}
	return path, nil
}

func readLimited(path string, limit int64) ([]byte, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, ioFailure(err)
	}
	defer file.Close()
	data, err := io.ReadAll(io.LimitReader(file, limit+1))
	if err != nil {
		return nil, ioFailure(err)
	}
	if int64(len(data)) > limit {
		return nil, failure("updates_package_invalid", "bundled metadata exceeds its size limit")
	}
	return data, nil
}

// verifyExecutable checks the thin Mach-O or PE header before accepting the
// publisher's target label.
func verifyExecutable(path, target string) error {
	file, err := os.Open(path)
	if err != nil {
		return ioFailure(err)
	}
	defer file.Close()
	header := make([]byte, 64)
	if _, err := io.ReadFull(file, header); err != nil {
		return failure("updates_package_invalid", "the packaged executable is incomplete")
	}
	arm := strings.HasPrefix(target, "aarch64-")
	valid := false
	if strings.HasSuffix(target, "apple-darwin") {
		cpu := uint32(0x01000007)
		if arm {
			cpu = 0x0100000c
		}
		valid = binary.LittleEndian.Uint32(header) == 0xfeedfacf && binary.LittleEndian.Uint32(header[4:]) == cpu
	} else {
		if string(header[:2]) != "MZ" {
			return failure("updates_package_invalid", "the packaged executable is not a PE image")
		}
		pe := make([]byte, 6)
		if _, err := file.ReadAt(pe, int64(binary.LittleEndian.Uint32(header[60:]))); err != nil {
			return failure("updates_package_invalid", "the PE header is incomplete")
		}
		machine := uint16(0x8664)
		if arm {
			machine = 0xaa64
		}
		valid = string(pe[:4]) == "PE\x00\x00" && binary.LittleEndian.Uint16(pe[4:]) == machine
	}
	if !valid {
		return failure("updates_platform", "the packaged executable architecture differs from signed metadata")
	}
	return nil
}
