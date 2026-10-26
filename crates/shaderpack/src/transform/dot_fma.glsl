/// Shifts a low/high word pair left without relying on 64-bit shader integers.
uvec2 iris_dot_left(uvec2 v, int n) {
    if (n == 0) return v;
    if (n >= 32) return uvec2(0u, v.x << uint(n - 32));
    return uvec2(v.x << uint(n), (v.y << uint(n)) | (v.x >> uint(32 - n)));
}

/// Shifts right, preserving discarded nonzero bits in the low sticky bit.
uvec2 iris_dot_right(uvec2 v, int n) {
    if (n <= 0) return iris_dot_left(v, -n);
    if (n >= 64) return uvec2(uint(any(notEqual(v, uvec2(0u)))), 0u);
    if (n >= 32) {
        int k = n - 32;
        uint lost = v.x;
        if (k > 0) lost |= v.y << uint(32 - k);
        return uvec2((v.y >> uint(k)) | uint(lost != 0u), 0u);
    }
    return uvec2((v.x >> uint(n)) | (v.y << uint(32 - n)) |
                 uint((v.x << uint(32 - n)) != 0u), v.y >> uint(n));
}

/// Finds the highest set bit of a nonzero low/high word pair.
int iris_dot_top(uvec2 v) {
    return v.y != 0u ? 32 + findMSB(v.y) : findMSB(v.x);
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
    if (ea == 255u || eb == 255u || ec == 255u) {
        if ((ea == 255u && ma != 0u) || (eb == 255u && mb != 0u) ||
            (ec == 255u && mc != 0u) ||
            (ea == 255u && (ub & 2147483647u) == 0u) ||
            (eb == 255u && (ua & 2147483647u) == 0u)) return uintBitsToFloat(2143289344u);
        if (ea == 255u || eb == 255u) {
            if (ec == 255u && sign != cs) return uintBitsToFloat(2143289344u);
            return uintBitsToFloat(sign | 2139095040u);
        }
        return c;
    }
    if (ea != 0u) ma |= 8388608u;
    if (eb != 0u) mb |= 8388608u;
    if (ec != 0u) mc |= 8388608u;
    if (ma == 0u || mb == 0u) {
        if (mc != 0u) return c;
        return uintBitsToFloat(sign & cs);
    }
    uvec2 p = iris_dot_product(ma, mb);
    int pt = iris_dot_top(p);
    int pe = int(max(ea, 1u)) + int(max(eb, 1u)) - 300 + pt;
    // Align the leading bits at bit 62, leaving one carry bit and at least 15 exact low bits.
    p = iris_dot_left(p, 62 - pt);
    uvec2 q = uvec2(0u);
    int exponent = pe;
    if (mc != 0u) {
        int ct = findMSB(mc);
        int ce = int(max(ec, 1u)) - 150 + ct;
        q = iris_dot_left(uvec2(mc, 0u), 62 - ct);
        exponent = max(pe, ce);
        // Discarded bits only occur when the exponents are too far apart for cancellation.
        p = iris_dot_right(p, exponent - pe);
        q = iris_dot_right(q, exponent - ce);
    }
    exponent -= 62;
    uvec2 value;
    if (sign == cs) {
        uint low = p.x + q.x;
        value = uvec2(low, p.y + q.y + uint(low < p.x));
    } else {
        if (p.y < q.y || (p.y == q.y && p.x < q.x)) {
            uvec2 swap = p; p = q; q = swap;
            sign = cs;
        }
        value = uvec2(p.x - q.x, p.y - q.y - uint(p.x < q.x));
        if (all(equal(value, uvec2(0u)))) return 0.0;
    }
    int shift = max(iris_dot_top(value) - 23, -149 - exponent);
    // Keep guard, round and sticky bits until the single nearest-even rounding below.
    uint rounded = iris_dot_right(value, shift - 3).x;
    uint mantissa = (rounded >> 3u) + uint((rounded & 7u) > 4u ||
                                            ((rounded & 7u) == 4u && (rounded & 8u) != 0u));
    int output_exp = exponent + shift + 150;
    if (mantissa >= 16777216u) { mantissa >>= 1u; output_exp++; }
    if (mantissa < 8388608u) output_exp = 0;
    if (output_exp >= 255) return uintBitsToFloat(sign | 2139095040u);
    return uintBitsToFloat(sign | (uint(output_exp) << 23u) | (mantissa & 8388607u));
}

/// Rounds the initial component product, preserving the sign of a zero product.
float iris_dot_mul(float a, float b) {
    uint sign = (floatBitsToUint(a) ^ floatBitsToUint(b)) & 2147483648u;
    return iris_dot_fma(a, b, uintBitsToFloat(sign));
}
