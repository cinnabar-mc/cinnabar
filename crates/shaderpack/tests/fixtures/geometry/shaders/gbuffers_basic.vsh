#version 330 compatibility
out vec4 surfaceTint;
void main() {
    gl_Position = ftransform();
    surfaceTint = gl_Color;
}
