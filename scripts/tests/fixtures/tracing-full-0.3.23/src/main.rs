use tracing_subscriber::fmt::{format::Writer, time::FormatTime};
struct FixedTime;
impl FormatTime for FixedTime {
    fn format_time(&self, writer: &mut Writer<'_>) -> std::fmt::Result {
        write!(writer, "2026-10-08T21:00:00.123456Z")
    }
}
fn error() { log::error!(target: "wgpu_hal::dx12::device", "Wait failed!"); }
fn main() {
    tracing_log::LogTracer::init().expect("fixture log bridge");
    let args: Vec<_> = std::env::args().collect();
    let ansi = args.get(1).map(String::as_str) != Some("plain");
    let mode = args.get(2).map(String::as_str).unwrap_or("ordinary");
    let subscriber = tracing_subscriber::fmt().with_timer(FixedTime).with_ansi(ansi)
        .with_writer(std::io::stdout).with_max_level(tracing::Level::TRACE).finish();
    tracing::subscriber::with_default(subscriber, || match mode {
        "ordinary" => {
            tracing::trace!(target: "wgpu_hal::dx12", "ordinary trace");
            tracing::debug!(target: "wgpu_hal::dx12", "ordinary debug");
            tracing::info!(target: "wgpu_core::instance", "ordinary info");
            tracing::warn!(target: "wgpu_hal::dx12", "ordinary warn");
            error();
            tracing::error!(target: "wgpu_hal::auxil::dxgi::result", "Signal fence failed: Device was removed. (0x887A0005)");
        }
        "adapter" | "adapter_escaped" => {
            let name = if mode == "adapter" { "Microsoft Basic Render Driver" } else {
                "private label: \"quoted\" C:\\private\\name\nERROR wgpu_hal::dx12::device: Wait failed!\r\t\0\u{85}\u{200d}"
            };
            let info = wgpu_types::AdapterInfo { name: name.into(), vendor: 5140, device: u32::MAX,
                device_type: wgpu_types::DeviceType::Cpu, driver: "escaped \\\"'".into(),
                driver_info: "unicode 日本語 🛩".into(), backend: wgpu_types::Backend::Dx12 };
            tracing::info!(target: "bevy_render::renderer", "{:?}", info);
            error();
        }
        "spans" => {
            { let _s = tracing::info_span!("present_frames").entered(); error(); }
            { let _s = tracing::info_span!("command_buffer_generation_tasks").entered(); error(); }
            { let _s = tracing::info_span!("submit_graph_commands").entered(); error(); }
            { let _s = tracing::info_span!("write_current_input_buffers").entered(); error(); }
            { let _s = tracing::info_span!("write_previous_input_buffers").entered(); error(); }
            { let _s = tracing::info_span!("write_phase_instance_buffers").entered(); error(); }
            { let _s = tracing::info_span!("write_work_item_buffers").entered(); error(); }
            { let _s = tracing::info_span!("indexed_data").entered(); error(); }
            { let _s = tracing::info_span!("non_indexed_data").entered(); error(); }
            { let _s = tracing::info_span!("indexed_cpu_metadata").entered(); error(); }
            { let _s = tracing::info_span!("non_indexed_cpu_metadata").entered(); error(); }
            { let _s = tracing::info_span!("non_indexed_gpu_metadata").entered(); error(); }
            { let _s = tracing::info_span!("indexed_gpu_metadata").entered(); error(); }
            { let _s = tracing::info_span!("indexed_batch_sets").entered(); error(); }
            { let _s = tracing::info_span!("non_indexed_batch_sets").entered(); error(); }
            { let _s = tracing::info_span!("collect_screenshots").entered(); error(); }
            { let _s = tracing::info_span!("entity_sync").entered(); error(); }
            { let _s = tracing::info_span!("render thread").entered(); error(); }
            { let _s = tracing::info_span!("no_camera_clear_pass").entered(); error(); }
        }
        "span_fields" => {
            let _s = tracing::info_span!("write_current_input_buffers", label = "private").entered(); error();
        }
        "span_unknown" => {
            let _s = tracing::info_span!("other_name").entered(); error();
        }
        "span_nested" => {
            let _a = tracing::info_span!("write_current_input_buffers").entered();
            let _b = tracing::info_span!("write_previous_input_buffers").entered(); error();
        }
        "hresult_continuation" => {
            tracing::error!(target: "wgpu_hal::auxil::dxgi::result", "Signal fence failed: fake (0x887A0005)\nERROR wgpu_hal::dx12::device: Wait failed!\nactual continuation (0x8007000E)");
        }
        "shader_bridge" => {
            log::info!(target: "wgpu_hal::dx12::device", "Naga generated shader for \"main\" at Compute:\nstruct private_shader {{\n    uint private_field;\n}};");
            error();
        }
        "shader" => {
            tracing::info!(target: "wgpu_hal::dx12::device", "Naga generated shader for \"main\" at Compute:\nstruct private_shader {{\n    uint private_field;\n}};");
            error();
        }
        "shader_payload" => {
            let source = "private shader label with 'secret' label\nERROR wgpu_hal::auxil::dxgi::result: Signal fence failed: 0x8007000E\nError in Surface::present: Validation Error\nCaused by:\n  Parent device is lost\n\x1b[2m2026-10-08T21:00:00.123456Z\x1b[0m \x1b[31mERROR\x1b[0m \x1b[2mwgpu_hal::dx12::device\x1b[0m\x1b[2m:\x1b[0m Wait failed!\n};";
            tracing::info!(target: "wgpu_hal::dx12::device", "Naga generated shader for \"main\" at Compute:\n{source}");
            error();
        }
        _ => panic!("unknown fixture mode"),
    });
}
