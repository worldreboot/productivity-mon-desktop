@group(0) @binding(0) var source: texture_2d<f32>;
@vertex fn vs(@builtin(vertex_index) index:u32) -> @builtin(position) vec4f {
    let position=array<vec2f,3>(vec2f(-1.0,-1.0),vec2f(3.0,-1.0),vec2f(-1.0,3.0));
    return vec4f(position[index],0.0,1.0);
}
@fragment fn fs(@builtin(position) position:vec4f) -> @location(0) vec4f {
    let color=textureLoad(source,vec2i(position.xy),0);
    if color.a<=0.0 {return vec4f(0.0);}
    return vec4f(clamp(color.rgb/color.a,vec3f(0.0),vec3f(1.0)),color.a);
}
