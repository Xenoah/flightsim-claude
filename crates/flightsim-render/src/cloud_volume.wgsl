#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import flightsim::cloud_field::{cloud_field_coordinates, cloud_field_rg_filtered, cloud_field_density, cloud_field_vertical_profile}

struct CloudUniform {
    view_from_clip: mat4x4<f32>,
    world_from_view: mat4x4<f32>,
    ecef_x: vec4<f32>, ecef_y: vec4<f32>, ecef_z: vec4<f32>,
    camera_ecef_cells: vec4<f32>, drift: vec4<f32>,
    shell: vec4<f32>, layer: vec4<f32>, intersections: vec4<f32>,
    sun: vec4<f32>, light: vec4<f32>, viewport: vec4<f32>, samples: vec4<f32>, seed: vec4<u32>,
};
@group(0) @binding(0) var<uniform> cloud: CloudUniform;
#ifdef MULTISAMPLED
@group(0) @binding(1) var scene_depth: texture_depth_multisampled_2d;
#else
@group(0) @binding(1) var scene_depth: texture_depth_2d;
#endif
@group(0) @binding(2) var cloud_color: texture_2d<f32>;
@group(0) @binding(3) var cloud_guide: texture_2d<f32>;

const MAX_DISTANCE: f32 = 700000.0;

fn ray_direction(uv: vec2<f32>) -> vec3<f32> {
    let p = cloud.view_from_clip * vec4<f32>(uv * vec2<f32>(2.0,-2.0) + vec2<f32>(-1.0,1.0),1.0,1.0);
    return normalize((cloud.world_from_view * vec4<f32>(p.xyz,0.0)).xyz);
}
fn depth_at(pixel: vec2<i32>) -> f32 {
    let limit = vec2<i32>(textureDimensions(scene_depth)) - vec2<i32>(1);
    let p = clamp(pixel,vec2<i32>(0),limit);
#ifdef MULTISAMPLED
    var nearest = 0.0;
    // Bevy uses reverse Z: maximum is the nearest opaque sample.
    for (var sample = 0u; sample < textureNumSamples(scene_depth); sample += 1u) {
        nearest = max(nearest,textureLoad(scene_depth,p,i32(sample)));
    }
    return nearest;
#else
    return textureLoad(scene_depth,p,0);
#endif
}
fn scene_distance(uv: vec2<f32>, depth: f32) -> f32 {
    if depth <= 0.0 { return MAX_DISTANCE; }
    let p = cloud.view_from_clip * vec4<f32>(uv * vec2<f32>(2.0,-2.0) + vec2<f32>(-1.0,1.0),depth,1.0);
    return min(length(p.xyz/p.w),MAX_DISTANCE);
}
fn shell_roots(direction: vec3<f32>, c: f32) -> vec2<f32> {
    let b = dot(cloud.shell.xyz,direction)*cloud.shell.w;
    let discriminant = b*b-c;
    if discriminant < 0.0 { return vec2<f32>(MAX_DISTANCE,-1.0); }
    let root = sqrt(discriminant);
    // Stable quadratic roots avoid cancellation looking almost straight down.
    let q = -b-select(-root,root,b>=0.0);
    let other = c/select(0.000001,q,abs(q)>0.000001);
    return vec2<f32>(min(q,other),max(q,other));
}
fn altitude(relative: vec3<f32>) -> f32 {
    let difference = 2.0*cloud.shell.w*dot(cloud.shell.xyz,relative)+dot(relative,relative);
    let radius_at_sample = sqrt(max(cloud.shell.w*cloud.shell.w+difference,1.0));
    return cloud.layer.x+difference/(radius_at_sample+cloud.shell.w);
}
fn sample_density(relative: vec3<f32>, footprint: f32, near_detail: bool) -> f32 {
    let h = (altitude(relative)-cloud.layer.y)/cloud.layer.z;
    if h<=0.0 || h>=1.0 { return 0.0; }
    let delta = vec3<f32>(dot(cloud.ecef_x.xyz,relative),dot(cloud.ecef_y.xyz,relative),dot(cloud.ecef_z.xyz,relative));
    let ecef = cloud.camera_ecef_cells.xyz+delta*cloud.camera_ecef_cells.w;
    let coords = cloud_field_coordinates(ecef,cloud.drift.xyz);
    let lod=clamp(log2(max(footprint/2000.0,1.0)),0.0,6.0);
    let field = cloud_field_rg_filtered(coords,lod);
    let noise = field.x;
    // At full resolution threshold the original R noise exactly as the CPU.
    // Mips store averaged threshold-aware horizontal density in G. This is
    // a distant horizontal-density smoothing approximation. Far shapes
    // are intentionally an approximation; cover caching error<=0.5/255.
    let horizontal = mix(cloud_field_density(noise,cloud.intersections.w),field.y,smoothstep(0.0,1.0,lod));
    if horizontal<=0.0 { return 0.0; }
    var shape_height=h;
    // Spatially stable 3D erosion only modulates occupied columns, preserving
    // the calibrated plan-view support of the common field. Remove unresolved
    // fine detail as a ray segment grows; no temporal/noise history exists.
    let detail_weight = 1.0-smoothstep(180.0,1000.0,footprint);
    let detail = clamp(0.5+0.30*sin(h*14.0+field.x*19.0),0.0,1.0);
    var erosion = mix(0.8,0.42+0.58*smoothstep(0.25,0.72,detail),detail_weight);
    // One extra existing-texture lookup only on resolved near view samples.
    // Unlike the radial macro footprint, these coordinates retain true ECEF
    // height. The CPU reduces the camera phase before f32 conversion.
    if near_detail {
      let resolved=1.0-smoothstep(250.0,1000.0,footprint);
      let nearby=1.0-smoothstep(15000.0,60000.0,length(relative));
      let sculpt_weight=resolved*nearby;
      if sculpt_weight>0.0001 {
        let fine_coords=vec3<f32>(cloud.ecef_x.w,cloud.ecef_y.w,cloud.ecef_z.w)
            + delta*0.001;
        let fine_lod=clamp(log2(max(footprint/250.0,1.0)),0.0,2.0);
        let fine=cloud_field_rg_filtered(fine_coords,fine_lod).r;
        let bottom=0.04+0.06*(1.0-noise);
        let coarse_top=0.58+0.38*noise;
        let detailed_top=clamp(coarse_top+0.65*(fine-0.5),bottom+0.25,0.98);
        let top=mix(coarse_top,detailed_top,sculpt_weight);
        // Keep the original bottom and positive column support. The top stays
        // inside the source layer; no new cloud can appear in a macro clear cell.
        shape_height=bottom+(h-bottom)*(coarse_top-bottom)/(top-bottom);
        erosion=mix(erosion,0.35+0.65*smoothstep(0.25,0.75,fine),sculpt_weight);
      }
    }
    let vertical=cloud_field_vertical_profile(shape_height,noise);
    return horizontal*vertical*erosion;
}
fn solar_transmittance(relative: vec3<f32>, footprint: f32) -> f32 {
    if cloud.sun.w<0.0001 { return 0.0; }
    let count = u32(cloud.samples.w);
    // Shadow reach follows layer thickness, independent of scene shadow maps.
    // Samples increasingly span 0..2 layer thicknesses; exact bounded4/6 calls.
    let reach = min(cloud.layer.z*2.0,8000.0);
    var optical_depth = 0.0;
    var previous = 0.0;
    for (var i=0u;i<count;i+=1u) {
        let x=f32(i+1u)/f32(count);
        let end=reach*x*x;
        let step=end-previous;
        let p=relative+cloud.sun.xyz*(previous+0.5*step);
        optical_depth+=sample_density(p,max(footprint,step),false)*step*cloud.layer.w;
        previous=end;
    }
    return exp(-optical_depth);
}
struct RayOutput { @location(0) color: vec4<f32>, @location(1) guide: f32 };
@fragment
fn raymarch(in: FullscreenVertexOutput) -> RayOutput {
    let uv=in.uv;
    let pixel=cloud.viewport.xy+uv*cloud.viewport.zw;
    let half_footprint=0.45*cloud.viewport.zw/cloud.samples.xy;
    var z=depth_at(vec2<i32>(pixel));
    z=max(z,depth_at(vec2<i32>(pixel+half_footprint)));
    z=max(z,depth_at(vec2<i32>(pixel-half_footprint)));
    z=max(z,depth_at(vec2<i32>(pixel+vec2<f32>(half_footprint.x,-half_footprint.y))));
    z=max(z,depth_at(vec2<i32>(pixel+vec2<f32>(-half_footprint.x,half_footprint.y))));
    let opaque_distance=scene_distance(uv,z);
    var output: RayOutput;
    output.color=vec4<f32>(0.0);
    output.guide=opaque_distance;
    let direction=ray_direction(uv);
    let outer=shell_roots(direction,cloud.intersections.z);
    var start=max(outer.x,0.0);
    var end=min(outer.y,opaque_distance);
    let inner=shell_roots(direction,cloud.intersections.y);
    if cloud.layer.x<cloud.layer.y { start=max(start,inner.y); }
    else if inner.x>0.0 { end=min(end,inner.x); }
    let earth=shell_roots(direction,cloud.intersections.x);
    if earth.x>0.0 { end=min(end,earth.x); }
    if end<=start || start>=MAX_DISTANCE { return output; }
    end=min(end,MAX_DISTANCE);
    let count=u32(cloud.samples.z);
    let span=end-start;
    // Exponential quadrature keeps the near edge resolved on long horizon
    // chords; step-dependent density filtering suppresses unresolved erosion.
    let curvature=mix(0.0,3.0,smoothstep(5000.0,70000.0,span));
    let exponential_denominator=max(exp(curvature)-1.0,0.000001);
    var previous=start;
    var transmittance=1.0;
    var radiance=vec3<f32>(0.0);
    let sun_alignment=max(dot(direction,cloud.sun.xyz),0.0);
    let rim=0.38+0.62*pow(sun_alignment,8.0);
    for(var i=0u;i<count;i+=1u) {
        let x=f32(i+1u)/f32(count);
        let distribution=select(x,(exp(curvature*x)-1.0)/exponential_denominator,curvature>0.001);
        let next=start+span*distribution;
        let step=next-previous;
        let distance=previous+step*0.5;
        let relative=direction*distance;
        let footprint=max(step,distance*2.0/max(cloud.samples.y,1.0));
        let density=sample_density(relative,footprint,true)*(1.0-smoothstep(650000.0,700000.0,distance));
        if density>0.0001 {
            let alpha=1.0-exp(-density*step*cloud.layer.w);
            let sun_transmission=solar_transmittance(relative,footprint);
            let h=clamp((altitude(relative)-cloud.layer.y)/cloud.layer.z,0.0,1.0);
            let ambient=vec3<f32>(0.65,0.76,1.0)*cloud.light.w*(0.30+0.70*h);
            let scattered=ambient+cloud.light.xyz*(0.18+rim)*sun_transmission;
            // Simple distance haze for the volume itself, after Bevy's sky has
            // already been composed. The sky's scene radiance is not fogged twice.
            let aerial=exp(-distance*0.000007);
            let haze=vec3<f32>(0.21,0.32,0.48)*cloud.sun.w;
            radiance+=transmittance*alpha*mix(haze,scattered,aerial);
            transmittance*=1.0-alpha;
            if transmittance<0.008 { break; }
        }
        previous=next;
    }
    output.color=vec4<f32>(radiance,1.0-transmittance);
    return output;
}
fn cloud_guide_weight(guide: f32, current_distance: f32) -> f32 {
    let tolerance=max(0.25,current_distance*0.01);
    let behind=max(guide-current_distance,0.0);
    return 1.0-smoothstep(tolerance,tolerance*2.0,behind);
}
@fragment
fn composite(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let uv=(in.position.xy-cloud.viewport.xy)/cloud.viewport.zw;
    let current_distance=scene_distance(uv,depth_at(vec2<i32>(in.position.xy)));
    let coord=uv*cloud.samples.xy-vec2<f32>(0.5);
    let base=vec2<i32>(floor(coord));
    let fraction=fract(coord);
    var accumulated=vec4<f32>(0.0);
    var total=0.0;
    for(var y=0;y<2;y+=1) {
        for(var x=0;x<2;x+=1) {
            let p=clamp(base+vec2<i32>(x,y),vec2<i32>(0),vec2<i32>(cloud.samples.xy)-vec2<i32>(1));
            let guide=textureLoad(cloud_guide,p,0).x;
            // A sky cloud sample may never cover a foreground cockpit/terrain
            // sample. Both linear depth and bilinear position guide the upsample.
            // A nearer guide integrated a valid, shorter foreground path. Do
            // not reject it for a longer/sky pixel: that creates blue slits
            // through dense clouds at distant terrain boundaries. Reject only
            // samples whose integration extends behind this opaque pixel.
            let compatible=cloud_guide_weight(guide,current_distance);
            let weight=select(1.0-fraction.x,fraction.x,x==1)*select(1.0-fraction.y,fraction.y,y==1)*compatible;
            accumulated+=textureLoad(cloud_color,p,0)*weight;
            total+=weight;
        }
    }
    if total<0.00001 { return vec4<f32>(0.0); }
    return accumulated/total;
}
