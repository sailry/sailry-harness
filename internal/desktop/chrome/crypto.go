package chrome

import (
	"bytes"
	"crypto/aes"
	"crypto/cipher"
	"crypto/sha1"
	"crypto/sha256"
	"unicode/utf8"

	"golang.org/x/crypto/pbkdf2"
)

// key derives Chrome's macOS cookie key from the Safe Storage password.
func key(password []byte) []byte {
	return pbkdf2.Key(password, []byte("saltysalt"), 1003, 16, sha1.New)
}

// decrypt opens a v10 value: AES-128-CBC under key with an IV of spaces and
// PKCS #7 padding, whose plaintext starts with the SHA-256 of the cookie's
// host. The returned value is the caller's to overwrite.
func decrypt(key []byte, domain string, encrypted []byte) ([]byte, error) {
	encrypted, found := bytes.CutPrefix(encrypted, []byte("v10"))
	if !found || len(encrypted) == 0 || len(encrypted)%aes.BlockSize != 0 {
		return nil, ErrCipher
	}
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, ErrCipher
	}
	plain := make([]byte, len(encrypted))
	defer clear(plain)
	cipher.NewCBCDecrypter(block, bytes.Repeat([]byte{' '}, aes.BlockSize)).CryptBlocks(plain, encrypted)
	padding := int(plain[len(plain)-1])
	if padding == 0 || padding > aes.BlockSize || !bytes.Equal(plain[len(plain)-padding:], bytes.Repeat([]byte{byte(padding)}, padding)) {
		return nil, ErrCipher
	}
	hash := sha256.Sum256([]byte(domain))
	value, found := bytes.CutPrefix(plain[:len(plain)-padding], hash[:])
	if !found || !utf8.Valid(value) {
		return nil, ErrCipher
	}
	return bytes.Clone(value), nil
}
