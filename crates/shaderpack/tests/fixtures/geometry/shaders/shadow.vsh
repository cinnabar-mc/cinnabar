#version 450 compatibility
layout(location = 0) out vec4 expandedTint;
out vec4 uvPadding;
void main() {
    gl_Position = gl_Vertex;
    expandedTint = gl_Color;
    uvPadding = gl_MultiTexCoord0;
    uvPadding.w /= 1.0 + float(gl_InstanceID);
}
