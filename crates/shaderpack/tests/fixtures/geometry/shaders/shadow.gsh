#version 450 compatibility
layout(triangles) in;
layout(triangle_strip, max_vertices = 6) out;
layout(location = 0) in vec4 tint[], uvPadding[];
layout(location = 1) out vec4 expandedTint;
flat out int lastCorner;
out vec2 inheritedPadding;
void main() {
    for (int copy = 0; copy < 2; copy++) {
        for (int corner = 0; corner < 3; corner++) {
            gl_Position = gl_in[corner].gl_Position;
            gl_Position.x = gl_Position.x * 0.4 + (float(copy) * 2.0 - 1.0) * 0.5;
            expandedTint = tint[corner] * vec4(float(1-copy), float(copy), 0.0, 1.0);
            lastCorner = corner;
            inheritedPadding = uvPadding[corner].zw;
            EmitVertex();
        }
        EndPrimitive();
    }
}
