// ONNX Runtime: the engine that runs AI upscaling's models on Windows and Linux
// (src/platform/upscale.rs, src/platform/faces.rs; macOS has Core ML). The library is
// bundled (scripts/fetch-onnxruntime.mjs, tauri.windows.conf.json / tauri.linux.conf.json)
// and loaded the first time a model is needed.
// - Windows: models run on the GPU through DirectML (any DirectX 12 GPU, integrated
//   ones too); on the CPU when there is none, or when the GPU fails.
// - Linux: on the CPU.
// Telemetry is switched off: nothing about the user's pictures or computer leaves it.

#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use ort::environment::Environment;
use ort::session::Session;
use ort::value::TensorRef;

static LIBRARY_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Called once at startup with the bundled `onnxruntime/` resource folder
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn set_library_dir(dir: PathBuf) {
    let _ = LIBRARY_DIR.set(dir);
}

const LIBRARY: &str = if cfg!(windows) {
    "onnxruntime.dll"
} else if cfg!(target_os = "macos") {
    "libonnxruntime.dylib"
} else {
    "libonnxruntime.so"
};

const MISSING: &str = "The AI engine is missing. Please reinstall flipfile.";

/// Where the library is: the bundled resources, or (debug builds and tests) the folder
/// the fetch script writes to
fn library_dir() -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = LIBRARY_DIR.get().cloned().into_iter().collect();
    if cfg!(debug_assertions) || cfg!(test) {
        dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("onnxruntime"));
    }
    dirs.into_iter().find(|dir| dir.join(LIBRARY).is_file())
}

/// True when the engine can be loaded here (tests on a Mac skip it otherwise)
#[cfg(test)]
pub fn available() -> bool {
    library_dir().is_some()
}

fn environment() -> Result<&'static Environment, String> {
    static ENVIRONMENT: OnceLock<Result<Environment, String>> = OnceLock::new();
    ENVIRONMENT
        .get_or_init(|| {
            let dir = library_dir().ok_or(MISSING)?;
            #[cfg(windows)]
            load_directml(&dir);
            ort::init_from(dir.join(LIBRARY))
                .map_err(|e| format!("the AI engine could not be loaded ({e})"))?
                .with_name("flipfile")
                .with_telemetry(false)
                .build()
                .map_err(|e| format!("the AI engine could not start ({e})"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// ONNX Runtime finds DirectML by its name; Windows also keeps an older DirectML.dll in
/// System32 that would be found first. Loading the bundled one by its full path first
/// makes that name refer to it (Windows reuses a library already loaded under a name).
#[cfg(windows)]
fn load_directml(dir: &Path) {
    use windows::core::HSTRING;
    use windows::Win32::System::LibraryLoader::LoadLibraryW;
    let path = dir.join("DirectML.dll");
    if path.is_file() {
        // Kept loaded for the life of the app
        let _ = unsafe { LoadLibraryW(&HSTRING::from(path.as_os_str())) };
    }
}

/// Where a model runs
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Device {
    Gpu,
    Cpu,
}

/// One bundled model and its session once loaded. Loading takes a fraction of a second,
/// while a loaded model holds on to a lot of memory (the face model over 1 GB while it
/// runs), so models are let go of after every batch (`release`). One input at a time.
pub struct Model {
    file: &'static str,
    /// Whether it may run on the GPU (small models with changing input sizes run on the CPU)
    gpu: bool,
    session: Mutex<Option<(Session, Device)>>,
}

/// One output: its shape and values
pub type Output = (Vec<usize>, Vec<f32>);

impl Model {
    pub const fn new(file: &'static str, gpu: bool) -> Model {
        Model { file, gpu, session: Mutex::new(None) }
    }

    /// Runs the model on one input (`shape`, planar `values`); returns the `outputs` named.
    /// Should the GPU fail, the model is loaded on the CPU and runs again.
    pub fn run(&self, input: &str, shape: &[usize], values: &[f32], outputs: &[&str]) -> Result<Vec<Output>, String> {
        let mut session = self.session.lock().unwrap_or_else(|e| e.into_inner());
        if session.is_none() {
            *session = Some(self.load(self.gpu)?);
        }
        let (loaded, device) = session.as_mut().expect("loaded above");
        match predict(loaded, input, shape, values, outputs) {
            Err(_) if *device == Device::Gpu => {
                *session = Some(self.load(false)?);
                predict(&mut session.as_mut().expect("loaded above").0, input, shape, values, outputs)
            }
            result => result,
        }
    }

    /// Loads the model if it isn't yet, and says where it runs; a failure is left for
    /// `run` to report
    pub fn prepare(&self) -> Option<Device> {
        let mut session = self.session.lock().unwrap_or_else(|e| e.into_inner());
        if session.is_none() {
            *session = self.load(self.gpu).ok();
        }
        session.as_ref().map(|(_, device)| *device)
    }

    /// Lets go of the model and the memory it holds
    pub fn release(&self) {
        *self.session.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// From now on, runs on the CPU (the GPU gave an unusable result)
    pub fn use_cpu(&self) -> Result<(), String> {
        let mut session = self.session.lock().unwrap_or_else(|e| e.into_inner());
        if session.as_ref().is_none_or(|(_, device)| *device == Device::Gpu) {
            *session = Some(self.load(false)?);
        }
        Ok(())
    }

    fn load(&self, gpu: bool) -> Result<(Session, Device), String> {
        let path = super::upscale::model_path(self.file).ok_or("The AI models are missing. Please reinstall flipfile.")?;
        let environment = environment()?;
        let failed = |e: &dyn std::fmt::Display| format!("an AI model could not be loaded ({e})");
        #[cfg(windows)]
        if gpu {
            if let Ok(session) = load_on_gpu(environment, &path) {
                return Ok((session, Device::Gpu));
            }
        }
        let _ = gpu;
        // Without ONNX Runtime's memory arena, which keeps every allocation's peak to reuse
        // (1.6 GB instead of 1 GB for the face model)
        let cpu = ort::ep::CPU::default().with_arena_allocator(false).build();
        let session = Session::builder(environment)
            .map_err(|e| failed(&e))?
            .with_execution_providers([cpu])
            .map_err(|e| failed(&e))?
            .commit_from_file(&path)
            .map_err(|e| failed(&e))?;
        Ok((session, Device::Cpu))
    }
}

/// A session on the GPU through DirectML, or an error when there is no usable GPU
#[cfg(windows)]
fn load_on_gpu(environment: &Environment, path: &Path) -> Result<Session, String> {
    let failed = |e: &dyn std::fmt::Display| e.to_string();
    Session::builder(environment)
        .map_err(|e| failed(&e))?
        // DirectML needs both off
        .with_memory_pattern(false)
        .map_err(|e| failed(&e))?
        .with_parallel_execution(false)
        .map_err(|e| failed(&e))?
        .with_execution_providers([ort::ep::DirectML::default().build().error_on_failure()])
        .map_err(|e| failed(&e))?
        .commit_from_file(path)
        .map_err(|e| failed(&e))
}

fn predict(session: &mut Session, input: &str, shape: &[usize], values: &[f32], outputs: &[&str]) -> Result<Vec<Output>, String> {
    let failed = |e: &dyn std::fmt::Display| format!("the AI model failed ({e})");
    let tensor = TensorRef::from_array_view((shape, values)).map_err(|e| failed(&e))?;
    let results = session.run(ort::inputs![input => tensor]).map_err(|e| failed(&e))?;
    outputs
        .iter()
        .map(|&name| {
            let value = results.get(name).ok_or_else(|| failed(&format!("no output {name}")))?;
            let (shape, data) = value.try_extract_tensor::<f32>().map_err(|e| failed(&e))?;
            Ok((shape.iter().map(|&d| d.max(0) as usize).collect(), data.to_vec()))
        })
        .collect()
}
