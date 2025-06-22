use pdfium_render::prelude::*;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let pdfium = Pdfium::default();
    let document = pdfium.load_pdf_from_file("garden/pdf/data/b.pdf", None)?;

    // collect all highlight annotations
    let mut highlights = Vec::new();
    for (page_index, page) in document.pages().iter().enumerate() {
        for annotation in page.annotations().iter() {
            match annotation.annotation_type() {
                | PdfPageAnnotationType::Highlight => {
                    let bounds = annotation.bounds()?;
                    log::info!(
                        "{:?} @ {} & {:?} -- {:?}",
                        annotation.annotation_type(),
                        page_index + 1,
                        bounds,
                        page.text()?.for_annotation(&annotation)?
                    );
                    highlights.push(bounds);
                }
                | PdfPageAnnotationType::Unknown
                | PdfPageAnnotationType::Text
                | PdfPageAnnotationType::Link
                | PdfPageAnnotationType::FreeText
                | PdfPageAnnotationType::Line
                | PdfPageAnnotationType::Square
                | PdfPageAnnotationType::Circle
                | PdfPageAnnotationType::Polygon
                | PdfPageAnnotationType::Polyline
                | PdfPageAnnotationType::Underline
                | PdfPageAnnotationType::Squiggly
                | PdfPageAnnotationType::Strikeout
                | PdfPageAnnotationType::Stamp
                | PdfPageAnnotationType::Caret
                | PdfPageAnnotationType::Ink
                | PdfPageAnnotationType::Popup
                | PdfPageAnnotationType::FileAttachment
                | PdfPageAnnotationType::Sound
                | PdfPageAnnotationType::Movie
                | PdfPageAnnotationType::Widget
                | PdfPageAnnotationType::Screen
                | PdfPageAnnotationType::PrinterMark
                | PdfPageAnnotationType::TrapNet
                | PdfPageAnnotationType::Watermark
                | PdfPageAnnotationType::ThreeD
                | PdfPageAnnotationType::RichMedia
                | PdfPageAnnotationType::XfaWidget
                | PdfPageAnnotationType::Redacted => {
                    log::trace!(
                        "{:?} @ {} & {:?} -- {:?}",
                        annotation.annotation_type(),
                        page_index + 1,
                        annotation.bounds()?,
                        page.text()?.for_annotation(&annotation)?
                    );
                }
            }
        }
    }

    // render the first page
    use image::ImageFormat;
    let page = document
        .pages()
        .iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("No pages found"))?;
    let render_config = PdfRenderConfig::new()
        .set_target_width(2000)
        .rotate_if_landscape(PdfPageRenderRotation::Degrees90, true);
    let result = page
        .render_with_config(&render_config)? // Initializes a bitmap with the given configuration for this page ...
        .as_image() // ... renders it to an Image::DynamicImage ...
        .into_rgb8() // ... sets the correct color space ...
        .save_with_format(format!("export-test-page-{}.jpg", 0), ImageFormat::Jpeg); // ... and exports it to a JPEG.

    assert!(result.is_ok());
    Ok(())
}
