// AI upscaling (Photo > Tools > Upscale): the machine-learning half, one engine per
// system. Everything else (cutting images into tiles, faces, putting it all back
// together) is shared by every system (src/convert/upscale.rs, src/convert/faces.rs).
// - macOS: Core ML runs the bundled models (src-tauri/models, built by
//   scripts/upscale-model/convert.py) on the Neural Engine and the CPU.
// - Windows and Linux: ONNX Runtime (src/platform/onnx.rs) runs the same models, exported
//   by scripts/upscale-model/export_onnx.py; on Windows on the GPU (DirectML).
//
// Three models, all reading and writing planar RGB floats (all red values, then green,
// then blue):
// - two upscalers of a TILE x TILE tile, SCALE times larger, values 0 to 1: a fast one
//   (Real-ESRGAN compact) and a detailed one (Real-ESRGAN x4plus, about ten times slower);
// - a face restorer of a FACE x FACE face aligned to a template, values -1 to 1
//   (RestoreFormer++).

use std::path::PathBuf;
use std::sync::OnceLock;

/// The side of the tile the upscalers read
pub const TILE: usize = 256;
/// How much larger the upscalers make it
pub const SCALE: usize = 4;
/// The side of the aligned face the face model reads and returns
pub const FACE: usize = 512;

/// Which upscaler reads a tile
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Upscaler {
    /// Real-ESRGAN compact: quick, for large pictures and for detail already restored
    Fast,
    /// Real-ESRGAN x4plus: natural detail on photos, for small pictures
    Detailed,
}

static MODEL_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Called once at startup with the bundled `models/` resource folder
pub fn set_model_dir(dir: PathBuf) {
    let _ = MODEL_DIR.set(dir);
}

/// Where a model is: the bundled resources, or (debug builds and tests) the folder the
/// fetch script writes to. A Core ML model is a folder, an ONNX model a file.
pub(crate) fn model_path(file: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = MODEL_DIR.get().cloned().into_iter().collect();
    if cfg!(debug_assertions) || cfg!(test) {
        dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models"));
    }
    let present = |path: &PathBuf| if file.ends_with(".mlmodelc") { path.join("coremldata.bin").is_file() } else { path.is_file() };
    dirs.into_iter().map(|dir| dir.join(file)).find(present)
}

/// What each step takes here before any has been measured, in seconds: a tile of the
/// fast upscaler, a tile of the detailed one, a face. (On Windows, this loads the fast
/// model to learn whether there is a GPU.)
pub fn expected_seconds() -> [f32; 3] {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        // Neural Engine (M1 Pro); faces on the CPU
        [0.027, 0.3, 0.5]
    } else if cfg!(target_os = "macos") {
        // Intel: the GPU, at best
        [0.2, 3.0, 2.0]
    } else {
        #[cfg(not(target_os = "macos"))]
        return onnx_models::expected_seconds();
        #[allow(unreachable_code)]
        [0.6, 15.0, 4.0]
    }
}

/// Upscales one tile (3 x TILE x TILE values, 0 to 1) into 3 x (TILE*SCALE)^2
#[cfg(target_os = "macos")]
pub fn upscale_tile(upscaler: Upscaler, tile: &[f32]) -> Result<Vec<f32>, String> {
    match upscaler {
        Upscaler::Fast => coreml::FAST.run(tile),
        Upscaler::Detailed => coreml::DETAILED.run(tile),
    }
}

/// Restores one aligned face (3 x FACE x FACE values, -1 to 1), same size and range
#[cfg(target_os = "macos")]
pub fn restore_face(face: &[f32]) -> Result<Vec<f32>, String> {
    coreml::FACES.run(face)
}

/// Starts loading the face model on a thread of its own, so it is ready once the
/// picture's tiles are done
#[cfg(target_os = "macos")]
pub fn prepare_faces() {
    let _ = std::thread::Builder::new().name("face-model".into()).spawn(|| coreml::FACES.prepare());
}

#[cfg(not(target_os = "macos"))]
pub use onnx_models::{prepare_faces, restore_face, upscale_tile};

/// Lets go of the models after a batch: on Windows and Linux they load again in a
/// fraction of a second and hold a lot of memory. (Core ML models take seconds to load
/// and stay.)
pub fn release() {
    #[cfg(not(target_os = "macos"))]
    {
        onnx_models::release();
        super::yunet::release();
    }
}

/// The models on ONNX Runtime: what Windows and Linux use (and tests on a Mac, when the
/// engine has been fetched there)
#[cfg(any(not(target_os = "macos"), test))]
pub mod onnx_models {
    use super::{Upscaler, FACE, SCALE, TILE};
    use crate::platform::onnx::{Device, Model};

    static FAST: Model = Model::new("upscale.onnx", true);
    static DETAILED: Model = Model::new("upscale-large.onnx", true);
    static FACES: Model = Model::new("faces.onnx", true);

    pub fn upscale_tile(upscaler: Upscaler, tile: &[f32]) -> Result<Vec<f32>, String> {
        let model = match upscaler {
            Upscaler::Fast => &FAST,
            Upscaler::Detailed => &DETAILED,
        };
        planar(model.run("image", &[1, 3, TILE, TILE], tile, &["upscaled"])?, TILE * SCALE)
    }

    pub fn restore_face(face: &[f32]) -> Result<Vec<f32>, String> {
        let run = || planar(FACES.run("face", &[1, 3, FACE, FACE], face, &["restored"])?, FACE);
        let restored = run()?;
        // A GPU that computes it wrongly gives a flat image (as the Neural Engine does
        // with this model on a Mac): then the CPU does it
        if spread(&restored) < 0.02 && FACES.prepare() == Some(Device::Gpu) {
            FACES.use_cpu()?;
            return run();
        }
        Ok(restored)
    }

    /// Starts loading the face model on a thread of its own, so it is ready once the
    /// picture's tiles are done
    pub fn prepare_faces() {
        let _ = std::thread::Builder::new().name("face-model".into()).spawn(|| FACES.prepare());
    }

    #[cfg_attr(test, allow(dead_code))]
    pub fn release() {
        for model in [&FAST, &DETAILED, &FACES] {
            model.release();
        }
    }

    /// See super::expected_seconds: measured with ONNX Runtime on an M1 Pro's CPU; a
    /// GPU's guessed (integrated GPUs, the slowest likely, take about a tenth)
    pub fn expected_seconds() -> [f32; 3] {
        match FAST.prepare() {
            Some(Device::Gpu) => [0.1, 1.5, 0.8],
            _ => [0.6, 15.0, 4.0],
        }
    }

    /// The values of the one output, checked to be 3 planes of `side` x `side`
    fn planar(mut outputs: Vec<crate::platform::onnx::Output>, side: usize) -> Result<Vec<f32>, String> {
        let (shape, values) = outputs.pop().ok_or("the AI model gave no image")?;
        if shape != [1, 3, side, side] || values.len() != 3 * side * side {
            return Err(format!("the AI model gave an unexpected output {shape:?}"));
        }
        Ok(values)
    }

    /// The standard deviation of the values
    fn spread(values: &[f32]) -> f32 {
        let n = values.len().max(1) as f32;
        let mean = values.iter().sum::<f32>() / n;
        (values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / n).sqrt()
    }
}

#[cfg(target_os = "macos")]
mod coreml {
    use std::ptr::NonNull;
    use std::sync::Mutex;

    use block2::RcBlock;
    use objc2::rc::{autoreleasepool, Retained};
    use objc2::runtime::{AnyObject, ProtocolObject};
    use objc2::AnyThread;
    use objc2_core_ml::{
        MLComputeUnits, MLDictionaryFeatureProvider, MLFeatureProvider, MLModel, MLModelConfiguration, MLMultiArray,
        MLMultiArrayDataType,
    };
    use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSURL};

    use super::{model_path, FACE, SCALE, TILE};

    /// One bundled model: its file, its input and output (planar RGB squares), where it
    /// runs, and the model itself once loaded. Loading takes a moment, so a model stays
    /// loaded; it runs one input at a time (its hardware does one at a time anyway).
    pub struct Spec {
        file: &'static str,
        input: (&'static str, usize),
        output: (&'static str, usize),
        units: MLComputeUnits,
        model: Mutex<Option<Model>>,
    }

    struct Model(Retained<MLModel>);
    // Only ever used while its Spec's lock is held
    unsafe impl Send for Model {}

    pub static FAST: Spec = Spec::new("upscale.mlmodelc", ("image", TILE), ("upscaled", TILE * SCALE), MLComputeUnits::All);
    pub static DETAILED: Spec =
        Spec::new("upscale-large.mlmodelc", ("image", TILE), ("upscaled", TILE * SCALE), MLComputeUnits::All);
    // 8-bit weights: on the Neural Engine this model returns a flat grey image. On the
    // CPU it loads in 0.8 s (the GPU compiles for 4.8 s, every launch) and needs 270 MB
    // less memory, for 0.5 s a face instead of 0.2 s.
    pub static FACES: Spec = Spec::new("faces.mlmodelc", ("face", FACE), ("restored", FACE), MLComputeUnits::CPUOnly);

    impl Spec {
        const fn new(
            file: &'static str,
            input: (&'static str, usize),
            output: (&'static str, usize),
            units: MLComputeUnits,
        ) -> Spec {
            Spec { file, input, output, units, model: Mutex::new(None) }
        }

        pub fn run(&self, values: &[f32]) -> Result<Vec<f32>, String> {
            let side = self.input.1;
            if values.len() != 3 * side * side {
                return Err("upscaling: an input of the wrong size".into());
            }
            let mut model = self.model.lock().unwrap_or_else(|e| e.into_inner());
            // Conversions run on worker threads, which have no autorelease pool of their own
            autoreleasepool(|_| {
                if model.is_none() {
                    *model = Some(Model(self.load()?));
                }
                self.predict(&model.as_ref().expect("loaded above").0, values)
            })
        }

        /// Loads the model if it isn't yet; a failure is left for `run` to report
        pub fn prepare(&self) {
            let mut model = self.model.lock().unwrap_or_else(|e| e.into_inner());
            if model.is_none() {
                *model = autoreleasepool(|_| self.load()).ok().map(Model);
            }
        }

        fn load(&self) -> Result<Retained<MLModel>, String> {
            // The models are built for macOS 13 (convert.py); older systems refuse them
            if macos_major().is_some_and(|major| major < 13) {
                return Err("AI Upscale needs macOS 13 Ventura or later".into());
            }
            let path = model_path(self.file).ok_or("The AI models are missing. Please reinstall flipfile.")?;
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
            let config = unsafe { MLModelConfiguration::new() };
            unsafe { config.setComputeUnits(self.units) };
            unsafe { MLModel::modelWithContentsOfURL_configuration_error(&url, &config) }
                .map_err(|e| format!("an AI model could not be loaded ({})", e.localizedDescription()))
        }

        fn predict(&self, model: &MLModel, values: &[f32]) -> Result<Vec<f32>, String> {
            let failed = |e: Retained<objc2_foundation::NSError>| format!("the AI model failed ({})", e.localizedDescription());
            let (name, side) = self.input;

            // Handed over as it is: Core ML reads it in place during the prediction
            let mut input = values.to_vec();
            let pointer = NonNull::new(input.as_mut_ptr().cast()).expect("a vector is never null");
            let array = unsafe {
                MLMultiArray::initWithDataPointer_shape_dataType_strides_deallocator_error(
                    MLMultiArray::alloc(),
                    pointer,
                    &numbers(&[1, 3, side, side]),
                    MLMultiArrayDataType::Float32,
                    &numbers(&[3 * side * side, side * side, side, 1]),
                    None,
                )
            }
            .map_err(failed)?;
            let key = NSString::from_str(name);
            let value: &AnyObject = &array;
            let features = NSDictionary::from_slices(&[&*key], &[value]);
            let provider =
                unsafe { MLDictionaryFeatureProvider::initWithDictionary_error(MLDictionaryFeatureProvider::alloc(), &features) }
                    .map_err(failed)?;
            let result = unsafe { model.predictionFromFeatures_error(ProtocolObject::from_ref(&*provider)) }.map_err(failed)?;
            drop(provider);
            drop(array);
            drop(input);

            let output = unsafe { result.featureValueForName(&NSString::from_str(self.output.0)) }
                .and_then(|value| unsafe { value.multiArrayValue() })
                .ok_or("the AI model gave no image")?;
            read(&output, self.output.1)
        }
    }

    /// The major version of macOS (26 for macOS 26.1)
    fn macos_major() -> Option<u32> {
        let mut buffer = [0u8; 32];
        let mut len = buffer.len();
        let found = unsafe {
            libc::sysctlbyname(c"kern.osproductversion".as_ptr(), buffer.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0)
        } == 0;
        let version = std::str::from_utf8(buffer.get(..len)?).ok()?.trim_end_matches('\0');
        found.then(|| version.split('.').next()?.parse().ok()).flatten()
    }

    fn numbers(values: &[usize]) -> Retained<NSArray<NSNumber>> {
        let values: Vec<Retained<NSNumber>> = values.iter().map(|&v| NSNumber::new_usize(v)).collect();
        NSArray::from_retained_slice(&values)
    }

    /// The output as planar floats, whatever layout Core ML chose (the Neural Engine may
    /// pad its rows, or answer in half precision)
    fn read(output: &MLMultiArray, side: usize) -> Result<Vec<f32>, String> {
        let shape: Vec<usize> = unsafe { output.shape() }.iter().map(|n| n.as_usize()).collect();
        if shape != [1, 3, side, side] {
            return Err(format!("the AI model gave an unexpected output {shape:?}"));
        }
        let strides: Vec<usize> = unsafe { output.strides() }.iter().map(|n| n.as_usize()).collect();
        let kind = unsafe { output.dataType() };
        let half = if kind == MLMultiArrayDataType::Float32 {
            false
        } else if kind == MLMultiArrayDataType::Float16 {
            true
        } else {
            return Err("the AI model gave an unexpected output type".into());
        };
        // Filled inside the handler, which Objective-C may only call, not mutate
        let out = std::cell::RefCell::new(vec![0f32; 3 * side * side]);
        let copy = RcBlock::new(|bytes: NonNull<std::ffi::c_void>, _size: isize| {
            let at = |c: usize, y: usize, x: usize| c * strides[1] + y * strides[2] + x * strides[3];
            let mut out = out.borrow_mut();
            for c in 0..3 {
                for y in 0..side {
                    let row = &mut out[(c * side + y) * side..][..side];
                    for (x, value) in row.iter_mut().enumerate() {
                        *value = unsafe {
                            if half {
                                f16_to_f32(*bytes.cast::<u16>().as_ptr().add(at(c, y, x)))
                            } else {
                                *bytes.cast::<f32>().as_ptr().add(at(c, y, x))
                            }
                        };
                    }
                }
            }
        });
        unsafe { output.getBytesWithHandler(&copy) };
        drop(copy);
        Ok(out.into_inner())
    }

    /// IEEE half precision to single
    fn f16_to_f32(bits: u16) -> f32 {
        let sign = u32::from(bits >> 15) << 31;
        let exponent = u32::from((bits >> 10) & 0x1f);
        let mantissa = u32::from(bits & 0x3ff);
        let value = match (exponent, mantissa) {
            (0, 0) => sign,
            (0, _) => {
                // Subnormal: no implicit leading 1
                let magnitude = mantissa as f32 * 2f32.powi(-24);
                return if sign != 0 { -magnitude } else { magnitude };
            }
            (0x1f, _) => sign | 0x7f80_0000 | (mantissa << 13),
            _ => sign | ((exponent + 112) << 23) | (mantissa << 13),
        };
        f32::from_bits(value)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn half_precision() {
            assert_eq!(f16_to_f32(0x3c00), 1.0);
            assert_eq!(f16_to_f32(0xc000), -2.0);
            assert_eq!(f16_to_f32(0x3800), 0.5);
            assert_eq!(f16_to_f32(0x0000), 0.0);
            assert!((f16_to_f32(0x0001) - 5.96e-8).abs() < 1e-9);
        }

        #[test]
        fn this_macos_is_known() {
            assert!(macos_major().is_some_and(|major| major >= 11), "{:?}", macos_major());
        }
    }
}
