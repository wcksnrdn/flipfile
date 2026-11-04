// Minimal PDF writer: one JPEG image per page, no extra library needed.

use std::io::{self, Write};

pub struct Page {
    pub jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// A one-channel (grayscale) JPEG
    pub gray: bool,
}

/// Counts what has been written, for the cross-reference table.
struct Counting<'a, W: Write> {
    inner: &'a mut W,
    written: usize,
}

impl<W: Write> Write for Counting<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.written += n;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Writes the PDF to `out` page by page, so merging many photos never needs a second
/// in-memory copy of every page.
pub fn write_jpeg_pdf(pages: &[Page], out: &mut impl Write) -> io::Result<()> {
    // Object numbers: 1 catalog, 2 page tree, then per page: page, image, content.
    let page_obj = |i: usize| 3 + i * 3;

    let mut out = Counting { inner: out, written: 0 };
    let mut offsets = Vec::with_capacity(2 + pages.len() * 3);
    out.write_all(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n")?;

    offsets.push(out.written);
    out.write_all(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n")?;

    let kids: Vec<String> = (0..pages.len()).map(|i| format!("{} 0 R", page_obj(i))).collect();
    offsets.push(out.written);
    write!(out, "2 0 obj\n<< /Type /Pages /Kids [{}] /Count {} >>\nendobj\n", kids.join(" "), pages.len())?;

    for (i, page) in pages.iter().enumerate() {
        let (p, img, content_obj) = (page_obj(i), page_obj(i) + 1, page_obj(i) + 2);
        let pw = page.width as f32 * 0.75; // pixels -> points (96 dpi)
        let ph = page.height as f32 * 0.75;
        let content = format!("q {pw:.2} 0 0 {ph:.2} 0 0 cm /Im0 Do Q");
        let color_space = if page.gray { "DeviceGray" } else { "DeviceRGB" };

        offsets.push(out.written);
        write!(
            out,
            "{p} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.2} {ph:.2}] \
             /Resources << /XObject << /Im0 {img} 0 R >> >> /Contents {content_obj} 0 R >>\nendobj\n"
        )?;

        offsets.push(out.written);
        write!(
            out,
            "{img} 0 obj\n<< /Type /XObject /Subtype /Image /Width {} /Height {} \
             /ColorSpace /{color_space} /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            page.width,
            page.height,
            page.jpeg.len()
        )?;
        out.write_all(&page.jpeg)?;
        out.write_all(b"\nendstream\nendobj\n")?;

        offsets.push(out.written);
        write!(out, "{content_obj} 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n", content.len())?;
    }

    let xref = out.written;
    write!(out, "xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1)?;
    for o in &offsets {
        writeln!(out, "{o:010} 00000 n ")?;
    }
    write!(out, "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1)?;
    Ok(())
}

/// The whole PDF in memory (tests).
#[cfg(test)]
pub fn jpeg_pages_to_pdf(pages: &[Page]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pages.iter().map(|p| p.jpeg.len() + 600).sum::<usize>() + 200);
    write_jpeg_pdf(pages, &mut out).expect("writing to memory cannot fail");
    out
}
