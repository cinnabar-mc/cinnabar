package conn

import (
	"bytes"
	"crypto/aes"
	"crypto/cipher"
	"crypto/sha1"
	"encoding/hex"
	"testing"
)

// referenceCFB8 is CFB8 written the obvious way: encrypt the register, XOR its first byte,
// shift the ciphertext byte in.
func referenceCFB8(block cipher.Block, iv, src []byte, decrypt bool) []byte {
	register := append([]byte(nil), iv...)
	keystream := make([]byte, block.BlockSize())
	dst := make([]byte, len(src))
	for i, in := range src {
		block.Encrypt(keystream, register)
		dst[i] = in ^ keystream[0]
		feedback := dst[i]
		if decrypt {
			feedback = in
		}
		register = append(register[1:], feedback)
	}
	return dst
}

// testBytes returns n deterministic bytes that differ by seed.
func testBytes(n int, seed uint32) []byte {
	b := make([]byte, n)
	state := seed
	for i := range b {
		state = state*1664525 + 1013904223
		b[i] = byte(state >> 24)
	}
	return b
}

func mustHex(t *testing.T, s string) []byte {
	t.Helper()
	b, err := hex.DecodeString(s)
	if err != nil {
		t.Fatal(err)
	}
	return b
}

// TestCFB8StandardVector checks the CFB8-AES128 example of NIST SP 800-38A, section F.3.7.
func TestCFB8StandardVector(t *testing.T) {
	block, err := aes.NewCipher(mustHex(t, "2b7e151628aed2a6abf7158809cf4f3c"))
	if err != nil {
		t.Fatal(err)
	}
	iv := mustHex(t, "000102030405060708090a0b0c0d0e0f")
	plaintext := mustHex(t, "6bc1bee22e409f96e93d7e117393172aae2d")
	ciphertext := mustHex(t, "3b79424c9c0dd436bace9e0ed4586a4f32b9")

	got := make([]byte, len(plaintext))
	newCFB8(block, iv, false).XORKeyStream(got, plaintext)
	if !bytes.Equal(got, ciphertext) {
		t.Fatalf("encrypted to %x, want %x", got, ciphertext)
	}
	newCFB8(block, iv, true).XORKeyStream(got, ciphertext)
	if !bytes.Equal(got, plaintext) {
		t.Fatalf("decrypted to %x, want %x", got, plaintext)
	}
}

func TestCFB8MatchesReferenceAcrossCalls(t *testing.T) {
	secret := testBytes(16, 1)
	block, err := aes.NewCipher(secret)
	if err != nil {
		t.Fatal(err)
	}
	// Long enough to move the shift register's window several times.
	plaintext := testBytes(3*cfb8Window+123, 2)
	want := referenceCFB8(block, secret, plaintext, false)

	encrypt, decrypt, err := newStreamCiphers(secret)
	if err != nil {
		t.Fatal(err)
	}
	ciphertext := make([]byte, len(plaintext))
	// Uneven pieces: the stream must not depend on how it is cut into calls.
	for start, step := 0, 1; start < len(plaintext); step = step*3 + 1 {
		end := min(start+step, len(plaintext))
		encrypt.XORKeyStream(ciphertext[start:end], plaintext[start:end])
		start = end
	}
	if !bytes.Equal(ciphertext, want) {
		t.Fatal("ciphertext differs from the reference implementation")
	}
	if !bytes.Equal(referenceCFB8(block, secret, ciphertext, true), plaintext) {
		t.Fatal("reference decryption did not restore the plaintext")
	}
	decrypt.XORKeyStream(ciphertext, ciphertext)
	if !bytes.Equal(ciphertext, plaintext) {
		t.Fatal("in-place decryption did not restore the plaintext")
	}
}

func TestStreamCiphersRejectBadSecret(t *testing.T) {
	for _, size := range []int{0, 15, 24, 32} {
		if _, _, err := newStreamCiphers(make([]byte, size)); err == nil {
			t.Errorf("shared secret of %d bytes was accepted", size)
		}
	}
}

// TestSignedHexVectors uses the three widely published login hash examples: the SHA-1 of a
// name printed the way the client prints the server hash.
func TestSignedHexVectors(t *testing.T) {
	for name, want := range map[string]string{
		"Notch": "4ed1f46bbe04bc756bcb17c0c7ce3e4632f06a48",
		"jeb_":  "-7c9d5b0044c130109a5d7b5fb5c317c02b4e28c1",
		"simon": "88e16a1019277b15d58faf0541e11910eb756f6",
	} {
		digest := sha1.Sum([]byte(name))
		if got := SignedHex(digest[:]); got != want {
			t.Errorf("SignedHex(sha1(%q)) = %s, want %s", name, got, want)
		}
	}
}

func TestSignedHexEdges(t *testing.T) {
	for _, tc := range []struct {
		digest []byte
		want   string
	}{
		{[]byte{0x00, 0x00}, "0"},
		{[]byte{0x00, 0x0f}, "f"},
		{[]byte{0x7f, 0xff}, "7fff"},
		{[]byte{0x80, 0x00}, "-8000"},
		{[]byte{0xff, 0xff}, "-1"},
		{nil, "0"},
	} {
		if got := SignedHex(tc.digest); got != tc.want {
			t.Errorf("SignedHex(%x) = %s, want %s", tc.digest, got, tc.want)
		}
	}
}

func TestServerHashCoversIDSecretAndKey(t *testing.T) {
	secret, key := testBytes(16, 3), testBytes(162, 4)
	// An id character above U+00FF is hashed as a question mark, one below as its Latin-1 byte.
	digest := sha1.Sum(append(append([]byte{'s', 0xe9, '?'}, secret...), key...))
	if got, want := ServerHash("sé€", secret, key), SignedHex(digest[:]); got != want {
		t.Fatalf("ServerHash = %s, want %s", got, want)
	}
	// With an empty id and key the hash is that of the secret alone.
	notch := sha1.Sum([]byte("Notch"))
	if got := ServerHash("", []byte("Notch"), nil); got != SignedHex(notch[:]) {
		t.Fatalf("ServerHash over the secret alone = %s", got)
	}
}
