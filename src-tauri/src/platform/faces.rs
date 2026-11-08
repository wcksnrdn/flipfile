// Faces in a picture, for AI upscaling to restore them (src/convert/faces.rs): five
// landmarks per face, found by the operating system.
// - macOS: Vision's face landmarks (VNDetectFaceLandmarksRequest), the same detector
//   Photos uses.
// - Windows and Linux: YuNet (src/platform/yunet.rs), a small face detector on ONNX Runtime.

use image::RgbImage;

/// One face: its eyes (the one on the picture's left first), nose tip and mouth corners
/// (left first), in pixels from the top left
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub points: [[f32; 2]; 5],
}

#[cfg(target_os = "macos")]
pub fn find_faces(image: &RgbImage) -> Result<Vec<Face>, String> {
    // Conversions run on worker threads, which have no autorelease pool of their own
    objc2::rc::autoreleasepool(|_| find_in_pool(image))
}

#[cfg(not(target_os = "macos"))]
pub fn find_faces(image: &RgbImage) -> Result<Vec<Face>, String> {
    super::yunet::find_faces(image)
}

#[cfg(target_os = "macos")]
fn find_in_pool(image: &RgbImage) -> Result<Vec<Face>, String> {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::AnyThread;
    use objc2_core_foundation::{CGPoint, CGSize};
    use objc2_foundation::{NSArray, NSDictionary};
    use objc2_vision::{VNDetectFaceLandmarksRequest, VNFaceLandmarkRegion2D, VNImageOption, VNImageRequestHandler, VNRequest};

    let failed = |e: &dyn std::fmt::Debug| format!("face detection failed ({e:?})");
    let (w, h) = (f64::from(image.width()), f64::from(image.height()));
    let pixels = crate::platform::system_codec::cg_image(image.as_raw(), image.width(), image.height(), 3, true)
        .ok_or_else(|| failed(&"image"))?;
    let options: Retained<NSDictionary<VNImageOption, AnyObject>> = NSDictionary::new();
    let handler = unsafe { VNImageRequestHandler::initWithCGImage_options(VNImageRequestHandler::alloc(), &pixels, &options) };
    let request = unsafe { VNDetectFaceLandmarksRequest::new() };
    let requests: Retained<NSArray<VNRequest>> =
        NSArray::from_retained_slice(&[Retained::into_super(Retained::into_super(request.clone()))]);
    handler.performRequests_error(&requests).map_err(|e| failed(&e))?;

    // A region's points in pixels from the top left (Vision counts from the bottom left)
    let points = |region: Option<Retained<VNFaceLandmarkRegion2D>>| -> Vec<CGPoint> {
        let Some(region) = region else { return vec![] };
        let count = unsafe { region.pointCount() };
        let first = unsafe { region.pointsInImageOfSize(CGSize { width: w, height: h }) };
        (0..count).map(|i| unsafe { *first.as_ptr().add(i) }).map(|p| CGPoint { x: p.x, y: h - p.y }).collect()
    };
    let centre = |p: &[CGPoint]| {
        let n = p.len().max(1) as f64;
        [(p.iter().map(|q| q.x).sum::<f64>() / n) as f32, (p.iter().map(|q| q.y).sum::<f64>() / n) as f32]
    };

    let mut faces = vec![];
    for face in unsafe { request.results() }.map(|r| r.to_vec()).unwrap_or_default() {
        let Some(marks) = (unsafe { face.landmarks() }) else { continue };
        // Pupils where Vision finds them, else the middle of each eye's outline
        let eye = |pupil: Vec<CGPoint>, outline: Vec<CGPoint>| centre(if pupil.is_empty() { &outline } else { &pupil });
        let mut eyes = [
            eye(points(unsafe { marks.leftPupil() }), points(unsafe { marks.leftEye() })),
            eye(points(unsafe { marks.rightPupil() }), points(unsafe { marks.rightEye() })),
        ];
        eyes.sort_by(|a, b| a[0].total_cmp(&b[0]));
        // The nose tip: under the middle of the nose outline (its crest bends with a turned head)
        let nose = points(unsafe { marks.nose() });
        let lips = points(unsafe { marks.outerLips() });
        if nose.is_empty() || lips.is_empty() {
            continue;
        }
        let tip = [centre(&nose)[0], nose.iter().map(|p| p.y).fold(f64::MIN, f64::max) as f32];
        let left = lips.iter().min_by(|a, b| a.x.total_cmp(&b.x)).expect("not empty");
        let right = lips.iter().max_by(|a, b| a.x.total_cmp(&b.x)).expect("not empty");
        faces.push(Face { points: [eyes[0], eyes[1], tip, [left.x as f32, left.y as f32], [right.x as f32, right.y as f32]] });
    }
    Ok(faces)
}
