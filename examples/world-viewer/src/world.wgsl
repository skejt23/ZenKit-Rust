struct Camera { view_projection: mat4x4<f32>, eye: vec4<f32> }
@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var diffuse: texture_2d<f32>;
@group(1) @binding(1) var diffuse_sampler: sampler;

struct Vertex {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) model0: vec4<f32>,
    @location(5) model1: vec4<f32>,
    @location(6) model2: vec4<f32>,
    @location(7) model3: vec4<f32>,
}
struct Fragment {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) world: vec3<f32>,
}
@vertex fn vs_main(v: Vertex) -> Fragment {
    let model = mat4x4<f32>(v.model0, v.model1, v.model2, v.model3);
    let world = model * vec4<f32>(v.position, 1.0);
    var f: Fragment;
    f.clip = camera.view_projection * world;
    f.normal = (model * vec4<f32>(v.normal, 0.0)).xyz;
    f.uv = v.uv;
    f.color = v.color;
    f.world = world.xyz;
    return f;
}
@fragment fn fs_main(f: Fragment) -> @location(0) vec4<f32> {
    let texel = textureSample(diffuse, diffuse_sampler, f.uv) * f.color;
    if texel.a < 0.4 { discard; }
    let n = normalize(f.normal + vec3<f32>(0.00001));
    let light = 0.65 + 0.35 * abs(dot(n, normalize(vec3<f32>(0.4, 0.85, 0.3))));
    let fog = 1.0 - exp(-distance(f.world, camera.eye.xyz) / 6000.0);
    return vec4<f32>(mix(texel.rgb * light, vec3<f32>(0.16, 0.22, 0.29), fog), texel.a);
}
