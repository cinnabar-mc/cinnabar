package conn

import (
	"crypto/aes"
	"crypto/rand"
	"crypto/rsa"
	"crypto/sha1"
	"crypto/x509"
	"errors"
	"fmt"
	"math/big"
)

// sharedSecretSize is the size of the AES-128 key the client generates for a session.
const sharedSecretSize = 16

// ServerHash returns the hash both peers hand to the session service to prove they talk to
// each other: SHA-1 over the server id, the shared secret and the server's encoded public key,
// printed as a signed hexadecimal number.
func ServerHash(serverID string, sharedSecret, publicKey []byte) string {
	h := sha1.New()
	h.Write(latin1(serverID))
	h.Write(sharedSecret)
	h.Write(publicKey)
	return SignedHex(h.Sum(nil))
}

// SignedHex prints digest the way 1.7.10 prints the server hash: the bytes read as a
// two's-complement big-endian integer, in lower-case hexadecimal without leading zeros, with a
// minus sign when the top bit is set.
func SignedHex(digest []byte) string {
	value := new(big.Int).SetBytes(digest)
	if len(digest) > 0 && digest[0]&0x80 != 0 {
		value.Sub(value, new(big.Int).Lsh(big.NewInt(1), uint(8*len(digest))))
	}
	return value.Text(16)
}

// latin1 encodes s as ISO 8859-1, the charset 1.7.10 hashes the server id in. Characters
// outside it become question marks.
func latin1(s string) []byte {
	b := make([]byte, 0, len(s))
	for _, r := range s {
		if r > 0xff {
			r = '?'
		}
		b = append(b, byte(r))
	}
	return b
}

// newSharedSecret returns a fresh random session key.
func newSharedSecret() ([]byte, error) {
	secret := make([]byte, sharedSecretSize)
	if _, err := rand.Read(secret); err != nil {
		return nil, fmt.Errorf("generate shared secret: %w", err)
	}
	return secret, nil
}

// parsePublicKey decodes the RSA key of an EncryptionRequest from its X.509 form.
func parsePublicKey(der []byte) (*rsa.PublicKey, error) {
	parsed, err := x509.ParsePKIXPublicKey(der)
	if err != nil {
		return nil, fmt.Errorf("parse server public key: %w", err)
	}
	key, ok := parsed.(*rsa.PublicKey)
	if !ok {
		return nil, errors.New("parse server public key: not an RSA key")
	}
	return key, nil
}

// encryptForServer encrypts data with the server's key using PKCS #1 v1.5 padding.
func encryptForServer(key *rsa.PublicKey, data []byte) ([]byte, error) {
	encrypted, err := rsa.EncryptPKCS1v15(rand.Reader, key, data)
	if err != nil {
		return nil, fmt.Errorf("encrypt for server: %w", err)
	}
	return encrypted, nil
}

// newStreamCiphers returns the encrypting and decrypting halves of a session's AES/CFB8
// stream. 1.7.10 uses the shared secret as both key and initial vector.
func newStreamCiphers(sharedSecret []byte) (encrypt, decrypt *cfb8, err error) {
	block, err := aes.NewCipher(sharedSecret)
	if err != nil {
		return nil, nil, fmt.Errorf("set up stream cipher: %w", err)
	}
	if len(sharedSecret) != block.BlockSize() {
		return nil, nil, fmt.Errorf("set up stream cipher: shared secret is %d bytes, want %d", len(sharedSecret), block.BlockSize())
	}
	return newCFB8(block, sharedSecret, false), newCFB8(block, sharedSecret, true), nil
}
