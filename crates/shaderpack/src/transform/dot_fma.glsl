// Backend compilers inline every dot's copy of these steps, so they select results instead
// of branching: DXC spent most of a large pack's load on the branches. Each shift amount
// stays within 0..31, so no lane shifts by the word size.

/// Shifts a low/high word pair left by `n`, at most 63, without 64-bit shader integers.
uvec2 iris_dot_left(uvec2 v, uint n) {
    uint s = n & 31u;
    uvec2 near = uvec2(v.x << s, (v.y << s) | ((v.x >> 1u) >> (31u - s)));
    return mix(near, uvec2(0u, v.x << s), bvec2(n >= 32u));
}

/// Shifts right, or left by at most 63 when `n` is negative, preserving discarded nonzero
/// bits in the low sticky bit.
uvec2 iris_dot_right(uvec2 v, int n) {
    uint s = uint(n) & 31u;
    uint lost_y = (v.y << 1u) << (31u - s);
    uvec2 near = uvec2((v.x >> s) | lost_y | uint(((v.x << 1u) << (31u - s)) != 0u), v.y >> s);
    uvec2 far = uvec2((v.y >> s) | uint((v.x | lost_y) != 0u), 0u);
    uvec2 gone = uvec2(uint(any(notEqual(v, uvec2(0u)))), 0u);
    uvec2 right = mix(mix(near, far, bvec2(n >= 32)), gone, bvec2(n >= 64));
    return mix(right, iris_dot_left(v, uint(-n)), bvec2(n < 0));
}

/// Finds the highest set bit of a low/high word pair, or -1 when it is zero.
int iris_dot_top(uvec2 v) {
    return mix(findMSB(v.x), 32 + findMSB(v.y), v.y != 0u);
}

/// Multiplies two binary32 significands exactly into a low/high word pair.
uvec2 iris_dot_product(uint a, uint b) {
    uint low = (a & 65535u) * (b & 65535u);
    uint middle = (a >> 16u) * (b & 65535u) + (b >> 16u) * (a & 65535u);
    uint word = low + (middle << 16u);
    return uvec2(word, (a >> 16u) * (b >> 16u) + (middle >> 16u) + uint(word < low));
}

/// Computes binary32 a*b+c with one nearest-even rounding, including gradual underflow.
float iris_dot_fma(float a, float b, float c) {
    uint ua = floatBitsToUint(a), ub = floatBitsToUint(b), uc = floatBitsToUint(c);
    uint ea = (ua >> 23u) & 255u, eb = (ub >> 23u) & 255u, ec = (uc >> 23u) & 255u;
    uint ma = ua & 8388607u, mb = ub & 8388607u, mc = uc & 8388607u;
    uint sign = (ua ^ ub) & 2147483648u;
    uint cs = uc & 2147483648u;
    // Infinite or NaN operands: the canonical NaN, the product's infinity, or c.
    bool infinite_product = ea == 255u || eb == 255u;
    bool special = infinite_product || ec == 255u;
    bool invalid = (ea == 255u && ma != 0u) || (eb == 255u && mb != 0u) ||
                   (ec == 255u && mc != 0u) ||
                   (ea == 255u && (ub & 2147483647u) == 0u) ||
                   (eb == 255u && (ua & 2147483647u) == 0u) ||
                   (infinite_product && ec == 255u && sign != cs);
    uint special_bits = mix(mix(uc, sign | 2139095040u, infinite_product), 2143289344u, invalid);
    ma |= uint(ea != 0u) << 23u;
    mb |= uint(eb != 0u) << 23u;
    mc |= uint(ec != 0u) << 23u;
    // A zero product leaves c, or a zero that is negative only when both zeros are.
    bool zero_product = ma == 0u || mb == 0u;
    uint zero_bits = mix(sign & cs, uc, mc != 0u);
    uvec2 p = iris_dot_product(ma, mb);
    int pt = iris_dot_top(p);
    int pe = int(max(ea, 1u)) + int(max(eb, 1u)) - 300 + pt;
    // Align the leading bits at bit 62, leaving one carry bit and at least 15 exact low bits.
    p = iris_dot_left(p, uint(62 - pt));
    int ct = findMSB(mc);
    int ce = int(max(ec, 1u)) - 150 + ct;
    uvec2 q = iris_dot_left(uvec2(mc, 0u), uint(62 - ct));
    int exponent = mix(pe, max(pe, ce), mc != 0u);
    // Discarded bits only occur when the exponents are too far apart for cancellation.
    p = iris_dot_right(p, exponent - pe);
    q = iris_dot_right(q, exponent - ce);
    exponent -= 62;
    uint low = p.x + q.x;
    uvec2 sum = uvec2(low, p.y + q.y + uint(low < p.x));
    bool swap = p.y < q.y || (p.y == q.y && p.x < q.x);
    uvec2 larger = mix(p, q, bvec2(swap));
    uvec2 smaller = mix(q, p, bvec2(swap));
    uvec2 difference = uvec2(larger.x - smaller.x,
                             larger.y - smaller.y - uint(larger.x < smaller.x));
    bool same = sign == cs;
    uvec2 value = mix(difference, sum, bvec2(same));
    sign = mix(sign, cs, !same && swap);
    bool cancelled = !same && all(equal(difference, uvec2(0u)));
    int shift = max(iris_dot_top(value) - 23, -149 - exponent);
    // Keep guard, round and sticky bits until the single nearest-even rounding below.
    uint rounded = iris_dot_right(value, shift - 3).x;
    uint mantissa = (rounded >> 3u) + uint((rounded & 7u) > 4u ||
                                            ((rounded & 7u) == 4u && (rounded & 8u) != 0u));
    int output_exp = exponent + shift + 150;
    bool carry = mantissa >= 16777216u;
    mantissa >>= uint(carry);
    output_exp += int(carry);
    output_exp = mix(output_exp, 0, mantissa < 8388608u);
    uint bits = mix(sign | (uint(output_exp) << 23u) | (mantissa & 8388607u),
                    sign | 2139095040u, output_exp >= 255);
    bits = mix(bits, 0u, cancelled);
    bits = mix(bits, zero_bits, zero_product);
    return uintBitsToFloat(mix(bits, special_bits, special));
}

/// The zero a*b starts from: negative when the product is, so a zero product keeps its sign.
float iris_dot_zero(float a, float b) {
    return uintBitsToFloat((floatBitsToUint(a) ^ floatBitsToUint(b)) & 2147483648u);
}
