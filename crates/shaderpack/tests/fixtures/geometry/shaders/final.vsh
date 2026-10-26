#version 330 compatibility
out vec2 frameUv;
void main() {
    gl_Position = gl_Vertex;
    frameUv = gl_MultiTexCoord0.xy;
}
