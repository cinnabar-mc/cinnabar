// Rounds finite coordinates to binary16 with ties to even, regardless of GPU conversion modes.
fn ui_half_float(value: f32) -> f32 {
    let bits = bitcast<u32>(value);
    let sign = bits & 0x80000000u;
    let magnitude = bits & 0x7fffffffu;
    if magnitude < 0x38800000u {
        let scaled = abs(value) * 16777216.0;
        let integer = u32(scaled);
        let fraction = scaled - f32(integer);
        let up = fraction > 0.5 || (fraction == 0.5 && (integer & 1u) != 0u);
        let rounded = f32(integer + select(0u, 1u, up)) / 16777216.0;
        return bitcast<f32>(bitcast<u32>(rounded) | sign);
    }
    if magnitude >= 0x477ff000u {
        return bitcast<f32>(sign | 0x7f800000u);
    }
    let rounded = (magnitude + 0xfffu + ((magnitude >> 13u) & 1u)) & 0xffffe000u;
    return bitcast<f32>(sign | rounded);
}
