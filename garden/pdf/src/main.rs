use image::ImageFormat;
use pdfium_render::prelude::*;

fn main() -> anyhow::Result<()> {
    let pdfium = Pdfium::default();
    let document = pdfium.load_pdf_from_file("pdf/test.pdf", None)?;
    // render the first page
    let page = document
        .pages()
        .iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("No pages found"))?;
    let render_config = PdfRenderConfig::new()
        .set_target_width(5000)
        .rotate_if_landscape(PdfPageRenderRotation::Degrees90, true);
    let result = page
        .render_with_config(&render_config)? // Initializes a bitmap with the given configuration for this page ...
        .as_image() // ... renders it to an Image::DynamicImage ...
        .into_rgb8() // ... sets the correct color space ...
        .save_with_format(format!("export-test-page-{}.jpg", 0), ImageFormat::Jpeg); // ... and exports it to a JPEG.

    assert!(result.is_ok());
    Ok(())
}
