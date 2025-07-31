#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct PostProcessSettings {
    sss: u32,
}

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var texture_sampler: sampler;
@group(0) @binding(2) var<uniform> settings: PostProcessSettings;

fn rgb_to_hsv(c: vec3<f32>) -> vec3<f32> {
    let K = vec4<f32>(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
    let p = mix(vec4(c.bg, K.wz), vec4(c.gb, K.xy), f32(c.b < c.g));
    let q = mix(vec4(p.xyw, c.r), vec4(c.r, p.yzx), f32(p.x < c.r));

    let d = q.x - min(q.w, q.y);
    let e = 1.0e-10;

    return vec3(
        abs(q.z + (q.w - q.y) / (6.0 * d + e)),
        d / (q.x + e),
        q.x
    );
}

fn hsv_to_rgb(c: vec3<f32>) -> vec3<f32> {
    let K = vec4<f32>(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);

    let p = abs(fract(vec3(c.x) + K.xyz) * 6.0 - K.www);

    return c.z * mix(K.xxx, clamp(p - K.xxx, vec3(0.0), vec3(1.0)), c.y);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let base = textureSample(screen_texture, texture_sampler, in.uv);
    let size = vec2<f32>(textureDimensions(screen_texture));
    let pix = vec2<i32>(in.uv * size);

    // half values
    let thickness = 0.5;
    let gap = 0.0;
    let arm_length = 7.5;

    let dist_x = abs(f32(pix.x) + 0.5 - size.x / 2.0);
    let dist_y = abs(f32(pix.y) + 0.5 - size.y / 2.0);

    let vertical = dist_x <= thickness && dist_y > gap && dist_y <= gap + arm_length;
    let horizontal = dist_y <= thickness && dist_x > gap && dist_x <= gap + arm_length;

    if vertical || horizontal {
        return mix(base, vec4(1.0, 1.0, 1.0, 1.0), 0.8);
    }

    switch(settings.sss) {
        case 1u: {
            let bayer = array<array<f32,2>,2>(
                array<f32,2>(0.0, 2.0),
                array<f32,2>(3.0, 1.0)
            );

            let threshold = bayer[pix.y % 4][pix.x % 4] / 4.0 - 0.5;

            return vec4(round(base.rgb * 15.0 + threshold) / 15.0, base.a);
        }
        case 2u: {
            var hsv = rgb_to_hsv(base.rgb);

            hsv.y = min(hsv.y * 1.5, 1.0);

            return vec4(hsv_to_rgb(hsv), base.a);
        }
        case 3u: {
            let r = textureSample(screen_texture, texture_sampler, in.uv + vec2(20.0 / size.x, 0.0));
            let b = textureSample(screen_texture, texture_sampler, in.uv + vec2(-20.0 / size.x, 0.0));

            return vec4(r.r, base.g, b.b, base.a);
        }
        case 4u: {
            return textureSample(screen_texture, texture_sampler, vec2(in.uv.x, 1.0 - in.uv.y));
        }
        case 5u: {
            let off = 1.0 / size;

            let center = dot(base.rgb, vec3(0.3, 0.59, 0.11));
            let north = dot(textureSample(screen_texture, texture_sampler, in.uv + vec2(0, off.y)).rgb, vec3(0.3, 0.59, 0.11));
            let east = dot(textureSample(screen_texture, texture_sampler, in.uv + vec2(off.x, 0)).rgb, vec3(0.3, 0.59, 0.11));

            let edge = abs(center - north) + abs(center - east);
            return vec4(vec3(edge), base.a);
        }
        case 6u: {
            let blocks = 200.0;
            let col = textureSample(screen_texture, texture_sampler, floor(in.uv * blocks) / blocks).rgb;
            return vec4(floor(col * 8.0) / 8.0, base.a);
        }
        case 7u: {
            let blocks = 200.0;
            let col = textureSample(screen_texture, texture_sampler, floor(in.uv * blocks) / blocks).g;
            return vec4(0.0, col, 0.0, base.a);
        }
        case 8u: {
            let vignette = smoothstep(0.4, 0.0, distance(in.uv, vec2(0.5)));

            return vec4(base.rgb * vec3(1.0, 0.2, 0.2) * vignette, base.a);
        }
        default: {
            return base;
        }
    }
}
