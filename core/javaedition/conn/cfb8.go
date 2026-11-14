package conn

import "crypto/cipher"

// cfb8Window is how many feedback bytes the shift register buffers before it is moved back
// to the start of its backing array.
const cfb8Window = 4096

// cfb8 is the 8-bit cipher feedback mode 1.7.10 encrypts its connection with. Each byte is
// XORed with the first byte of the encrypted shift register, and the ciphertext byte is then
// shifted in. One value handles one direction of one stream.
type cfb8 struct {
	block   cipher.Block
	decrypt bool
	// register holds the shift register as a window sliding over a larger array, so shifting
	// in a byte is an append rather than a move.
	register []byte
	pos      int
	stream   []byte
}

// newCFB8 returns a CFB8 stream over block starting from iv, which must be one block long.
func newCFB8(block cipher.Block, iv []byte, decrypt bool) *cfb8 {
	size := block.BlockSize()
	c := &cfb8{
		block:    block,
		decrypt:  decrypt,
		register: make([]byte, size+cfb8Window),
		stream:   make([]byte, size),
	}
	copy(c.register, iv)
	return c
}

// XORKeyStream encrypts or decrypts src into dst, which may be the same slice. Calls continue
// the stream where the previous one stopped.
func (c *cfb8) XORKeyStream(dst, src []byte) {
	size := len(c.stream)
	for i, in := range src {
		if c.pos+size == len(c.register) {
			copy(c.register, c.register[c.pos:])
			c.pos = 0
		}
		c.block.Encrypt(c.stream, c.register[c.pos:c.pos+size])
		out := in ^ c.stream[0]
		feedback := out
		if c.decrypt {
			feedback = in
		}
		c.register[c.pos+size] = feedback
		c.pos++
		dst[i] = out
	}
}
